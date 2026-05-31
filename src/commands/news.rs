use std::collections::{HashMap, HashSet};

use anyhow::{bail, Context, Result};
use serde_json::json;

use crate::config::Config;
use crate::commands::news_sentiment;
use crate::commands::refresh::build_brave_news_queries;
use crate::data::{brave, rss};
use crate::db::backend::BackendConnection;
use crate::db::news_cache::{
    get_latest_news_filtered_backend, parse_news_source_independence_filter, NewsEntry,
};
use crate::db::news_topic_markets::{self, BoundNewsMarket};
use crate::db::rss_feed_health;

fn news_empty_diagnostics(backend: &BackendConnection) -> serde_json::Value {
    let rss_last_fetch =
        crate::db::news_cache::latest_fetched_at_by_source_type_backend(backend, "rss")
            .ok()
            .flatten();
    let brave_last_fetch =
        crate::db::news_cache::latest_fetched_at_by_source_type_backend(backend, "brave")
            .ok()
            .flatten();

    let likely_reason = match (rss_last_fetch.as_ref(), brave_last_fetch.as_ref()) {
        (None, None) => "news cache empty; refresh has not successfully stored RSS or Brave articles yet",
        (None, Some(_)) => "news cache has Brave articles only; RSS ingest may be failing or disabled",
        (Some(_), None) => "news cache has RSS articles only; Brave news is unavailable or unconfigured",
        (Some(_), Some(_)) => "news cache is filtered empty for the requested query or all cached articles expired",
    };

    json!({
        "articles": [],
        "status": "empty",
        "error": likely_reason,
        "diagnostics": {
            "rss_last_fetch": rss_last_fetch,
            "brave_last_fetch": brave_last_fetch,
        }
    })
}

/// Run the `pftui news` command.
///
/// In JSON mode, this always returns valid JSON and exit 0, even when the
/// database query fails or the news cache is empty. Errors are reported via
/// an `"error"` field in the JSON output and on stderr so agents can parse
/// the output reliably.
#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    config: &Config,
    source: Option<&str>,
    search: Option<&str>,
    hours: Option<i64>,
    breaking: bool,
    filter_independence: Option<&str>,
    limit: usize,
    with_sentiment: bool,
    json: bool,
) -> Result<()> {
    let independence_filter = filter_independence
        .map(parse_news_source_independence_filter)
        .transpose()?;

    let entries = if breaking {
        match fetch_live_entries(
            backend,
            config,
            source,
            search,
            hours,
            independence_filter.as_deref(),
            limit,
        ) {
            Ok(entries) => entries,
            Err(err) => {
                if json {
                    let error_json = json!({
                        "articles": [],
                        "error": format!("Failed to fetch live news: {err:#}"),
                        "live_fetch": true,
                    });
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&error_json).unwrap_or_else(|_| {
                            r#"{"articles":[],"error":"serialization failed","live_fetch":true}"#
                                .to_string()
                        })
                    );
                    eprintln!("warning: live news query failed: {err:#}");
                    return Ok(());
                }
                return Err(err.context("Failed to fetch live news"));
            }
        }
    } else {
        match get_latest_news_filtered_backend(
            backend,
            limit,
            source,
            None,
            search,
            hours,
            independence_filter.as_deref(),
        ) {
            Ok(entries) => entries,
            Err(err) => {
                if json {
                    // JSON mode: return valid JSON with error info, exit 0
                    let error_json = json!({
                        "articles": [],
                        "error": format!("Failed to fetch news: {err:#}")
                    });
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&error_json).unwrap_or_else(|_| {
                            r#"{"articles":[],"error":"serialization failed"}"#.to_string()
                        })
                    );
                    eprintln!("warning: news query failed: {err:#}");
                    return Ok(());
                }
                // Text mode: propagate the error normally
                return Err(err.context("Failed to fetch news from cache"));
            }
        }
    };

    if entries.is_empty() {
        if json {
            let payload = if breaking {
                json!({
                    "articles": [],
                    "status": "empty",
                    "error": "live news fetch returned no matching articles",
                    "live_fetch": true,
                })
            } else {
                news_empty_diagnostics(backend)
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| {
                    r#"{"articles":[],"status":"empty","error":"news empty"}"#.to_string()
                })
            );
        } else {
            if breaking {
                println!("No live news entries matched the requested filters.");
            } else {
                println!("No cached news entries. Run `pftui refresh` first.");
                let diagnostics = news_empty_diagnostics(backend);
                if let Some(reason) = diagnostics.get("error").and_then(|v| v.as_str()) {
                    println!("Reason: {reason}");
                }
                if let Some(rss_last_fetch) = diagnostics["diagnostics"]["rss_last_fetch"].as_str()
                {
                    println!("Last RSS fetch: {rss_last_fetch}");
                }
                if let Some(brave_last_fetch) =
                    diagnostics["diagnostics"]["brave_last_fetch"].as_str()
                {
                    println!("Last Brave fetch: {brave_last_fetch}");
                }
            }
        }
        return Ok(());
    }

    if json {
        if with_sentiment {
            print_json_with_sentiment(backend, &entries)?;
        } else {
            print_json(backend, &entries)?;
        }
    } else {
        print_table(&entries);
    }

    Ok(())
}

pub fn run_feeds_list(backend: &BackendConnection, config: &Config, json: bool) -> Result<()> {
    let feeds = rss::configured_feeds(config);
    let health = rss_feed_health::health_for_feeds_backend(backend, &feeds)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "feeds": health }))?
        );
    } else {
        println!(
            "{:<28} {:<11} {:<8} {:<8} Last Failure",
            "Feed", "Status", "Failures", "Category"
        );
        println!("{}", "-".repeat(82));
        for feed in health {
            let last_failure = feed
                .last_failure_reason
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect::<String>();
            println!(
                "{:<28} {:<11} {:<8} {:<8} {}",
                feed.feed_name,
                feed.status,
                feed.consecutive_failures,
                feed.category,
                last_failure
            );
        }
    }

    Ok(())
}

pub fn run_feeds_reset(backend: &BackendConnection, feed_id: &str, json: bool) -> Result<()> {
    rss_feed_health::reset_feed_backend(backend, feed_id)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "reset",
                "feed_id": feed_id,
            }))?
        );
    } else {
        println!("Reset RSS feed health for {}", feed_id);
    }
    Ok(())
}

pub fn run_sources_list(backend: &BackendConnection, json: bool) -> Result<()> {
    let sources = crate::db::news_cache::list_news_source_tiers_backend(backend)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "sources": sources }))?
        );
    } else {
        println!("{:<32} {:<5} Notes", "Domain", "Tier");
        println!("{}", "-".repeat(72));
        for source in sources {
            println!(
                "{:<32} {:<5} {}",
                source.domain,
                source.tier,
                source.notes.unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub fn run_sources_unclassified(
    backend: &BackendConnection,
    since: &str,
    min_articles: i64,
    json: bool,
) -> Result<()> {
    if min_articles <= 0 {
        bail!("--min-articles must be positive");
    }
    let window_days = parse_source_window_days(since)?;
    let domains = crate::db::news_cache::list_unclassified_news_sources_backend(
        backend,
        window_days,
        min_articles,
    )?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "window_days": window_days,
                "min_articles": min_articles,
                "domains": domains,
            }))?
        );
    } else {
        println!(
            "Unclassified source domains over {}d (min articles: {})",
            window_days, min_articles
        );
        println!(
            "{:<36} {:>8} {:<20} {:<20}",
            "Domain", "Articles", "First seen", "Last seen"
        );
        println!("{}", "-".repeat(90));
        for domain in domains {
            println!(
                "{:<36} {:>8} {:<20} {:<20}",
                domain.domain,
                domain.article_count,
                domain.first_seen_at.unwrap_or_default(),
                domain.last_seen_at.unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub fn run_sources_stats(backend: &BackendConnection, since: &str, json: bool) -> Result<()> {
    let window_days = parse_source_window_days(since)?;
    let stats = crate::db::news_cache::news_source_stats_backend(backend, window_days)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&json!({ "stats": stats }))?);
    } else {
        println!("News source stats over {}d", stats.window_days);
        println!(
            "Articles: {} | explicit: {} ({:.2}%) | inferred: {} ({:.2}%)",
            stats.total_articles,
            stats.explicit_articles,
            stats.explicit_pct,
            stats.inferred_articles,
            stats.inferred_pct
        );
        println!();
        println!("Top domains");
        println!(
            "{:<36} {:>8} {:<5} {:<9} {:<20}",
            "Domain", "Articles", "Tier", "Inferred", "Last seen"
        );
        println!("{}", "-".repeat(88));
        for domain in stats.top_domains {
            println!(
                "{:<36} {:>8} {:<5} {:<9} {:<20}",
                domain.domain,
                domain.article_count,
                domain.source_tier,
                domain.source_tier_inferred,
                domain.last_seen_at.unwrap_or_default()
            );
        }
        println!();
        println!("Top unclassified");
        println!(
            "{:<36} {:>8} {:<20}",
            "Domain", "Articles", "Last seen"
        );
        println!("{}", "-".repeat(68));
        for domain in stats.top_unclassified {
            println!(
                "{:<36} {:>8} {:<20}",
                domain.domain,
                domain.article_count,
                domain.last_seen_at.unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn parse_source_window_days(raw: &str) -> Result<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(7);
    }
    let lower = trimmed.to_ascii_lowercase();
    let value = lower
        .strip_suffix("days")
        .map(str::trim)
        .or_else(|| lower.strip_suffix('d').map(str::trim))
        .unwrap_or(&lower);
    let days = value
        .parse::<i64>()
        .with_context(|| format!("invalid --since duration '{raw}'; expected a value like 7d"))?;
    if days <= 0 {
        bail!("--since must be a positive duration like 7d");
    }
    Ok(days)
}

pub fn run_sources_set(
    backend: &BackendConnection,
    domain: &str,
    tier: i64,
    notes: Option<&str>,
    json: bool,
) -> Result<()> {
    let source = crate::db::news_cache::set_news_source_tier_backend(backend, domain, tier, notes)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "set",
                "source": source,
            }))?
        );
    } else {
        println!("Set {} to tier {}", source.domain, source.tier);
    }
    Ok(())
}

pub fn run_sources_remove(backend: &BackendConnection, domain: &str, json: bool) -> Result<()> {
    let removed = crate::db::news_cache::remove_news_source_tier_backend(backend, domain)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": if removed { "removed" } else { "missing" },
                "domain": domain,
            }))?
        );
    } else if removed {
        println!("Removed source tier mapping for {}", domain);
    } else {
        println!("No source tier mapping found for {}", domain);
    }
    Ok(())
}

pub fn run_topics_list(backend: &BackendConnection, json: bool) -> Result<()> {
    let topics = news_topic_markets::list_topic_markets_backend(backend)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "topics": topics }))?
        );
    } else {
        println!(
            "{:<18} {:<34} {:<34} Notes",
            "Topic", "Primary Market", "Secondary Market"
        );
        println!("{}", "-".repeat(104));
        for topic in topics {
            println!(
                "{:<18} {:<34} {:<34} {}",
                topic.topic,
                topic.primary_market_id,
                topic.secondary_market_id.unwrap_or_default(),
                topic.notes.unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub fn run_topics_set(
    backend: &BackendConnection,
    topic: &str,
    primary_market_id: &str,
    secondary_market_id: Option<&str>,
    notes: Option<&str>,
    json: bool,
) -> Result<()> {
    let mapping = news_topic_markets::set_topic_market_backend(
        backend,
        topic,
        primary_market_id,
        secondary_market_id,
        notes,
    )?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": "set",
                "topic": mapping,
            }))?
        );
    } else {
        println!(
            "Set {} -> {}",
            mapping.topic, mapping.primary_market_id
        );
    }
    Ok(())
}

pub fn run_topics_remove(backend: &BackendConnection, topic: &str, json: bool) -> Result<()> {
    let removed = news_topic_markets::remove_topic_market_backend(backend, topic)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "status": if removed { "removed" } else { "missing" },
                "topic": topic,
            }))?
        );
    } else if removed {
        println!("Removed news topic market mapping for {}", topic);
    } else {
        println!("No news topic market mapping found for {}", topic);
    }
    Ok(())
}

fn fetch_live_entries(
    backend: &BackendConnection,
    config: &Config,
    source: Option<&str>,
    search: Option<&str>,
    hours: Option<i64>,
    independence_filter: Option<&[crate::db::news_cache::NewsSourceIndependence]>,
    limit: usize,
) -> Result<Vec<NewsEntry>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let disabled_feed_ids = rss_feed_health::disabled_feed_ids_backend(backend).unwrap_or_default();
    let rss_feeds: Vec<_> = rss::configured_feeds(config)
        .into_iter()
        .filter(|feed| !disabled_feed_ids.contains(&feed.feed_id()))
        .collect();
    let brave_key = config.brave_api_key.as_deref().unwrap_or("").trim().to_string();
    let brave_queries = if brave_key.is_empty() {
        Vec::new()
    } else {
        build_brave_news_queries(backend, config).unwrap_or_default()
    };

    let (rss_report, brave_results) = rt.block_on(async {
        let rss_fut = rss::fetch_all_feeds_detailed(&rss_feeds);
        let brave_fut = async {
            let mut items = Vec::new();
            for query in &brave_queries {
                if let Ok(results) = brave::brave_news_search(&brave_key, query, Some("pd"), 10).await
                {
                    items.extend(results);
                }
            }
            items
        };
        tokio::join!(rss_fut, brave_fut)
    });

    let mut entries = Vec::new();
    let fetched_at = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    for success in &rss_report.successes {
        rss_feed_health::record_feed_success_backend(backend, &success.feed_name)?;
    }
    for err in &rss_report.errors {
        rss_feed_health::record_feed_failure_backend(backend, &err.feed_name, &err.error)?;
    }

    for item in rss_report.items {
        let classification =
            crate::db::news_cache::classify_news_source_backend(backend, &item.url, &item.source)?;
        let independence = crate::db::news_cache::classify_news_source_independence(
            &item.title,
            &item.source,
            item.description.as_deref(),
            &[],
        );
        let title = item.title;
        let description = item.description.unwrap_or_default();
        let category = item.category.as_str().to_string();
        let extra_snippets = Vec::new();
        let topic = news_topic_markets::classify_news_topic(
            &title,
            &category,
            Some(&description),
            &extra_snippets,
        );
        let entry = NewsEntry {
            id: 0,
            title,
            url: item.url,
            source: item.source,
            source_type: "rss".to_string(),
            symbol_tag: None,
            source_domain: classification.domain,
            source_tier: classification.tier,
            source_tier_inferred: classification.inferred,
            source_independence: independence,
            description,
            extra_snippets,
            category,
            topic,
            published_at: item.published_at,
            fetched_at: fetched_at.clone(),
        };
        cache_live_entry(backend, &entry)?;
        entries.push(entry);
    }

    for item in brave_results {
        let source = item.source.unwrap_or_else(|| "Brave".to_string());
        let classification =
            crate::db::news_cache::classify_news_source_backend(backend, &item.url, &source)?;
        let independence = crate::db::news_cache::classify_news_source_independence(
            &item.title,
            &source,
            Some(&item.description),
            &item.extra_snippets,
        );
        let title = item.title;
        let description = item.description;
        let extra_snippets = item.extra_snippets;
        let category = "markets".to_string();
        let topic = news_topic_markets::classify_news_topic(
            &title,
            &category,
            Some(&description),
            &extra_snippets,
        );
        let entry = NewsEntry {
            id: 0,
            title,
            url: item.url,
            source,
            source_type: "brave".to_string(),
            symbol_tag: None,
            source_domain: classification.domain,
            source_tier: classification.tier,
            source_tier_inferred: classification.inferred,
            source_independence: independence,
            description,
            extra_snippets,
            category,
            topic,
            published_at: chrono::Utc::now().timestamp(),
            fetched_at: fetched_at.clone(),
        };
        cache_live_entry(backend, &entry)?;
        entries.push(entry);
    }

    Ok(filter_entries(
        entries,
        source,
        search,
        hours,
        independence_filter,
        limit,
    ))
}

fn cache_live_entry(backend: &BackendConnection, entry: &NewsEntry) -> Result<()> {
    crate::db::news_cache::insert_news_with_source_type_backend(
        backend,
        &entry.title,
        &entry.url,
        &entry.source,
        &entry.source_type,
        entry.symbol_tag.as_deref(),
        &entry.category,
        entry.published_at,
        Some(&entry.description),
        &entry.extra_snippets,
    )
}

fn filter_entries(
    entries: Vec<NewsEntry>,
    source: Option<&str>,
    search: Option<&str>,
    hours: Option<i64>,
    independence_filter: Option<&[crate::db::news_cache::NewsSourceIndependence]>,
    limit: usize,
) -> Vec<NewsEntry> {
    let source = source.map(str::to_lowercase);
    let search = search.map(str::to_lowercase);
    let cutoff = hours.map(|value| chrono::Utc::now().timestamp() - (value * 3600));
    let mut seen_urls = HashSet::new();

    let mut filtered: Vec<_> = entries
        .into_iter()
        .filter(|entry| {
            source
                .as_ref()
                .is_none_or(|wanted| entry.source.to_lowercase() == *wanted)
        })
        .filter(|entry| {
            search.as_ref().is_none_or(|wanted| {
                entry.title.to_lowercase().contains(wanted)
                    || entry.description.to_lowercase().contains(wanted)
            })
        })
        .filter(|entry| cutoff.is_none_or(|min_ts| entry.published_at >= min_ts))
        .filter(|entry| {
            independence_filter.is_none_or(|values| {
                values.contains(&entry.source_independence)
            })
        })
        .filter(|entry| seen_urls.insert(entry.url.clone()))
        .collect();

    filtered.sort_by_key(|b| std::cmp::Reverse(b.published_at));
    filtered.truncate(limit);
    filtered
}

/// Print news entries as a formatted table.
fn print_table(entries: &[NewsEntry]) {
    if entries.is_empty() {
        println!("No matching news entries found.");
        return;
    }

    // Calculate column widths
    let title_width = 80;
    let source_width = 20;
    let time_width = 16;

    // Print header
    println!(
        "{:<title$}  {:<source$}  {:<time$}",
        "Title",
        "Source",
        "Time",
        title = title_width,
        source = source_width,
        time = time_width,
    );
    println!(
        "{}",
        "─".repeat(title_width + source_width + time_width + 4)
    );

    // Print rows
    for entry in entries {
        let title = if entry.title.len() > title_width {
            format!("{}...", &entry.title[..title_width - 3])
        } else {
            entry.title.clone()
        };

        let time_str = format_timestamp(entry.published_at);

        println!(
            "{:<title$}  {:<source$}  {:<time$}",
            title,
            entry.source,
            time_str,
            title = title_width,
            source = source_width,
            time = time_width,
        );
    }

    println!("\nTotal: {} articles", entries.len());
}

/// Format Unix timestamp as relative time or date string.
fn format_timestamp(ts: i64) -> String {
    let dt = chrono::DateTime::from_timestamp(ts, 0).unwrap_or_else(chrono::Utc::now);
    let now = chrono::Utc::now();
    let diff = now.signed_duration_since(dt);

    if diff.num_minutes() < 60 {
        format!("{}m ago", diff.num_minutes())
    } else if diff.num_hours() < 24 {
        format!("{}h ago", diff.num_hours())
    } else if diff.num_days() < 7 {
        format!("{}d ago", diff.num_days())
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

/// Print news entries as JSON array.
///
/// Always outputs valid JSON. If serialization fails (shouldn't happen with
/// serde_json::Value), falls back to an empty array.
fn bound_markets_for_entries(
    backend: &BackendConnection,
    entries: &[NewsEntry],
) -> HashMap<String, Vec<BoundNewsMarket>> {
    let topics = entries
        .iter()
        .map(|entry| entry.topic.clone())
        .collect::<Vec<_>>();
    news_topic_markets::bound_markets_by_topic_backend(backend, &topics).unwrap_or_else(|err| {
        eprintln!("warning: failed to bind news topics to prediction markets: {err:#}");
        HashMap::new()
    })
}

fn news_entry_json(entry: &NewsEntry, bound_markets: &[BoundNewsMarket]) -> serde_json::Value {
    json!({
        "id": entry.id,
        "title": entry.title,
        "url": entry.url,
        "source": entry.source,
        "source_type": entry.source_type,
        "symbol_tag": entry.symbol_tag,
        "source_domain": entry.source_domain,
        "source_tier": entry.source_tier,
        "source_tier_inferred": entry.source_tier_inferred,
        "source_independence": entry.source_independence.as_str(),
        "description": entry.description,
        "extra_snippets": entry.extra_snippets,
        "category": entry.category,
        "topic": entry.topic,
        "bound_markets": bound_markets,
        "published_at": entry.published_at,
        "fetched_at": entry.fetched_at,
    })
}

fn print_json(backend: &BackendConnection, entries: &[NewsEntry]) -> Result<()> {
    let bound_by_topic = bound_markets_for_entries(backend, entries);
    let json_entries: Vec<_> = entries
        .iter()
        .map(|entry| {
            news_entry_json(
                entry,
                bound_by_topic
                    .get(&entry.topic)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            )
        })
        .collect();

    match serde_json::to_string_pretty(&json_entries) {
        Ok(output) => println!("{output}"),
        Err(err) => {
            // Fallback: still output valid JSON so agents don't break
            eprintln!("warning: news JSON serialization failed: {err}");
            println!("[]");
        }
    }
    Ok(())
}

/// Print news entries as JSON with sentiment scores.
fn print_json_with_sentiment(backend: &BackendConnection, entries: &[NewsEntry]) -> Result<()> {
    let scored = news_sentiment::score_all(entries);
    let bound_by_topic = bound_markets_for_entries(backend, entries);
    let json_entries: Vec<_> = scored
        .iter()
        .map(|s| {
            json!({
                "id": s.entry.id,
                "title": s.entry.title,
                "url": s.entry.url,
                "source": s.entry.source,
                "source_type": s.entry.source_type,
                "symbol_tag": s.entry.symbol_tag,
                "source_domain": s.entry.source_domain,
                "source_tier": s.entry.source_tier,
                "source_tier_inferred": s.entry.source_tier_inferred,
                "source_independence": s.entry.source_independence.as_str(),
                "description": s.entry.description,
                "extra_snippets": s.entry.extra_snippets,
                "category": s.entry.category,
                "topic": s.entry.topic,
                "bound_markets": bound_by_topic
                    .get(&s.entry.topic)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                "published_at": s.entry.published_at,
                "fetched_at": s.entry.fetched_at,
                "sentiment_score": s.score,
                "sentiment_label": s.label.as_str(),
                "bullish_hits": s.bullish_hits,
                "bearish_hits": s.bearish_hits,
            })
        })
        .collect();

    match serde_json::to_string_pretty(&json_entries) {
        Ok(output) => println!("{output}"),
        Err(err) => {
            eprintln!("warning: news JSON serialization failed: {err}");
            println!("[]");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use crate::db::news_cache::{insert_news, NewsSourceIndependence};

    fn to_backend(conn: rusqlite::Connection) -> BackendConnection {
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn test_format_timestamp() {
        let now = chrono::Utc::now().timestamp();
        let five_min_ago = now - 300;
        let two_hours_ago = now - 7200;
        let yesterday = now - 86400;

        assert!(format_timestamp(five_min_ago).contains("m ago"));
        assert!(format_timestamp(two_hours_ago).contains("h ago"));
        assert!(format_timestamp(yesterday).contains("d ago"));
    }

    #[test]
    fn test_print_json_empty() {
        let entries: Vec<NewsEntry> = vec![];
        let backend = to_backend(crate::db::open_in_memory());
        let result = print_json(&backend, &entries);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_json_valid_entries() {
        let entries = vec![
            NewsEntry {
                id: 1,
                title: "Test headline".to_string(),
                url: "https://example.com/test".to_string(),
                source: "TestSource".to_string(),
                source_type: "rss".to_string(),
                symbol_tag: None,
                source_domain: "example.com".to_string(),
                source_tier: 3,
                source_tier_inferred: true,
                source_independence: NewsSourceIndependence::Independent,
                description: "A test article".to_string(),
                extra_snippets: vec!["snippet1".to_string()],
                category: "markets".to_string(),
                topic: "equities".to_string(),
                published_at: 1709610000,
                fetched_at: "2024-03-05 10:00:00".to_string(),
            },
            NewsEntry {
                id: 2,
                title: "Another headline".to_string(),
                url: "https://example.com/test2".to_string(),
                source: "OtherSource".to_string(),
                source_type: "brave".to_string(),
                symbol_tag: Some("BTC".to_string()),
                source_domain: "example.com".to_string(),
                source_tier: 3,
                source_tier_inferred: true,
                source_independence: NewsSourceIndependence::Independent,
                description: "".to_string(),
                extra_snippets: vec![],
                category: "crypto".to_string(),
                topic: "crypto".to_string(),
                published_at: 1709620000,
                fetched_at: "2024-03-05 12:00:00".to_string(),
            },
        ];

        let backend = to_backend(crate::db::open_in_memory());
        let result = print_json(&backend, &entries);
        assert!(result.is_ok());
    }

    #[test]
    fn news_entry_json_includes_source_tier_fields() {
        let entry = NewsEntry {
            id: 1,
            title: "Fed headline".to_string(),
            url: "https://reuters.com/markets/fed".to_string(),
            source: "Reuters".to_string(),
            source_type: "rss".to_string(),
            symbol_tag: None,
            source_domain: "reuters.com".to_string(),
            source_tier: 1,
            source_tier_inferred: false,
            source_independence: NewsSourceIndependence::Wire,
            description: String::new(),
            extra_snippets: vec![],
            category: "macro".to_string(),
            topic: "fed-policy".to_string(),
            published_at: 1709610000,
            fetched_at: "2024-03-05 10:00:00".to_string(),
        };

        let bound = BoundNewsMarket {
            role: "primary".to_string(),
            contract_id: "fed-contract".to_string(),
            available: true,
            exchange: Some("polymarket".to_string()),
            event_id: Some("evt-fed".to_string()),
            event_title: Some("Fed decision".to_string()),
            question: Some("Will the Fed hold?".to_string()),
            category: Some("economics".to_string()),
            probability: Some(0.62),
            probability_pct: Some(62.0),
            volume_24h: Some(1000.0),
            liquidity: Some(5000.0),
            end_date: None,
            updated_at: Some(1711670000),
        };
        let payload = news_entry_json(&entry, &[bound]);
        assert_eq!(payload["source_domain"], "reuters.com");
        assert_eq!(payload["source_tier"], 1);
        assert_eq!(payload["source_tier_inferred"], false);
        assert_eq!(payload["source_independence"], "wire");
        assert_eq!(payload["topic"], "fed-policy");
        assert_eq!(payload["bound_markets"][0]["contract_id"], "fed-contract");
        assert_eq!(payload["bound_markets"][0]["probability_pct"], 62.0);
    }

    #[test]
    fn test_run_empty_cache_json() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        // JSON mode with empty cache should return Ok (exit 0), not error
        let result = run(&backend, &crate::config::Config::default(), None, None, None, false, None, 20, false, true);
        assert!(result.is_ok(), "JSON mode should not fail on empty cache");
    }

    #[test]
    fn test_news_empty_diagnostics_reports_empty_status() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        let diagnostics = news_empty_diagnostics(&backend);
        assert_eq!(diagnostics["status"], "empty");
        assert!(diagnostics["articles"]
            .as_array()
            .is_some_and(|articles| articles.is_empty()));
        assert!(diagnostics["error"]
            .as_str()
            .is_some_and(|msg| msg.contains("news cache empty")));
    }

    #[test]
    fn test_run_empty_cache_text() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        let result = run(&backend, &crate::config::Config::default(), None, None, None, false, None, 20, false, false);
        assert!(result.is_ok(), "Text mode should not fail on empty cache");
    }

    #[test]
    fn test_run_with_entries_json() {
        let conn = crate::db::open_in_memory();

        insert_news(
            &conn,
            "Bitcoin hits $100k",
            "https://example.com/btc-100k",
            "CoinDesk",
            "crypto",
            chrono::Utc::now().timestamp(),
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &crate::config::Config::default(), None, None, None, false, None, 20, false, true);
        assert!(result.is_ok(), "JSON mode should succeed with entries");
    }

    #[test]
    fn test_run_with_entries_text() {
        let conn = crate::db::open_in_memory();

        insert_news(
            &conn,
            "Gold surges past $3000",
            "https://example.com/gold-3k",
            "Reuters",
            "commodities",
            chrono::Utc::now().timestamp(),
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &crate::config::Config::default(), None, None, None, false, None, 20, false, false);
        assert!(result.is_ok(), "Text mode should succeed with entries");
    }

    #[test]
    fn test_run_with_sentiment_json() {
        let conn = crate::db::open_in_memory();

        insert_news(
            &conn,
            "Markets surge on stimulus hopes",
            "https://example.com/surge",
            "Reuters",
            "markets",
            chrono::Utc::now().timestamp(),
        )
        .unwrap();

        let backend = to_backend(conn);
        let result = run(&backend, &crate::config::Config::default(), None, None, None, false, None, 20, true, true);
        assert!(
            result.is_ok(),
            "JSON mode with sentiment should succeed with entries"
        );
    }

    #[test]
    fn filter_entries_applies_filters_and_dedupes() {
        let now = chrono::Utc::now().timestamp();
        let entries = vec![
            NewsEntry {
                id: 0,
                title: "Fed holds rates steady".to_string(),
                url: "https://example.com/fed".to_string(),
                source: "Reuters".to_string(),
                source_type: "rss".to_string(),
                symbol_tag: None,
                source_domain: "example.com".to_string(),
                source_tier: 3,
                source_tier_inferred: true,
                source_independence: NewsSourceIndependence::Wire,
                description: "Fresh macro update".to_string(),
                extra_snippets: Vec::new(),
                category: "macro".to_string(),
                topic: "fed-policy".to_string(),
                published_at: now,
                fetched_at: "2026-04-22 12:00:00".to_string(),
            },
            NewsEntry {
                id: 0,
                title: "Fed holds rates steady".to_string(),
                url: "https://example.com/fed".to_string(),
                source: "Reuters".to_string(),
                source_type: "brave".to_string(),
                symbol_tag: None,
                source_domain: "example.com".to_string(),
                source_tier: 3,
                source_tier_inferred: true,
                source_independence: NewsSourceIndependence::Wire,
                description: "Duplicate URL".to_string(),
                extra_snippets: Vec::new(),
                category: "macro".to_string(),
                topic: "fed-policy".to_string(),
                published_at: now - 60,
                fetched_at: "2026-04-22 12:00:00".to_string(),
            },
            NewsEntry {
                id: 0,
                title: "Bitcoin rallies".to_string(),
                url: "https://example.com/btc".to_string(),
                source: "CoinDesk".to_string(),
                source_type: "rss".to_string(),
                symbol_tag: None,
                source_domain: "example.com".to_string(),
                source_tier: 3,
                source_tier_inferred: true,
                source_independence: NewsSourceIndependence::Independent,
                description: "Crypto move".to_string(),
                extra_snippets: Vec::new(),
                category: "crypto".to_string(),
                topic: "crypto".to_string(),
                published_at: now - 7200,
                fetched_at: "2026-04-22 12:00:00".to_string(),
            },
        ];

        let filtered = filter_entries(
            entries.clone(),
            Some("Reuters"),
            Some("fed"),
            Some(1),
            None,
            10,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].source, "Reuters");
        assert!(filtered[0].title.contains("Fed"));

        let wire_only = filter_entries(
            entries,
            None,
            None,
            None,
            Some(&[NewsSourceIndependence::Wire]),
            10,
        );
        assert_eq!(wire_only.len(), 1);
        assert_eq!(wire_only[0].source_independence, NewsSourceIndependence::Wire);
    }

    #[test]
    fn feeds_reset_reenables_a_disabled_feed() {
        let backend = to_backend(crate::db::open_in_memory());
        for _ in 0..rss_feed_health::DISABLED_THRESHOLD {
            rss_feed_health::record_feed_failure_backend(
                &backend,
                "Bloomberg Commodities",
                "parse error",
            )
            .unwrap();
        }

        run_feeds_reset(&backend, "Bloomberg Commodities", true).unwrap();

        let health = rss_feed_health::list_feed_health_backend(&backend).unwrap();
        let row = health
            .iter()
            .find(|row| row.feed_id == "Bloomberg Commodities")
            .unwrap();
        assert_eq!(row.status, "active");
        assert_eq!(row.consecutive_failures, 0);
    }
}
