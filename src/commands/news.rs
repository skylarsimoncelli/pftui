use anyhow::Result;
use serde_json::json;

use crate::commands::news_sentiment;
use crate::db::backend::BackendConnection;
use crate::db::news_cache::{get_latest_news_backend, NewsEntry};

/// Run the `pftui news` command.
///
/// In JSON mode, this always returns valid JSON and exit 0, even when the
/// database query fails or the news cache is empty. Errors are reported via
/// an `"error"` field in the JSON output and on stderr so agents can parse
/// the output reliably.
pub fn run(
    backend: &BackendConnection,
    source: Option<&str>,
    search: Option<&str>,
    hours: Option<i64>,
    limit: usize,
    with_sentiment: bool,
    json: bool,
) -> Result<()> {
    let entries = match get_latest_news_backend(backend, limit, source, None, search, hours) {
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
    };

    if entries.is_empty() {
        if json {
            println!("[]");
        } else {
            println!("No cached news entries. Run `pftui refresh` first.");
        }
        return Ok(());
    }

    if json {
        if with_sentiment {
            print_json_with_sentiment(&entries)?;
        } else {
            print_json(&entries)?;
        }
    } else {
        print_table(&entries);
    }

    Ok(())
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
fn print_json(entries: &[NewsEntry]) -> Result<()> {
    let json_entries: Vec<_> = entries
        .iter()
        .map(|entry| {
            json!({
                "id": entry.id,
                "title": entry.title,
                "url": entry.url,
                "source": entry.source,
                "source_type": entry.source_type,
                "symbol_tag": entry.symbol_tag,
                "description": entry.description,
                "extra_snippets": entry.extra_snippets,
                "category": entry.category,
                "published_at": entry.published_at,
                "fetched_at": entry.fetched_at,
            })
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
fn print_json_with_sentiment(entries: &[NewsEntry]) -> Result<()> {
    let scored = news_sentiment::score_all(entries);
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
                "description": s.entry.description,
                "extra_snippets": s.entry.extra_snippets,
                "category": s.entry.category,
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
    use crate::db::news_cache::insert_news;

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
        let result = print_json(&entries);
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
                description: "A test article".to_string(),
                extra_snippets: vec!["snippet1".to_string()],
                category: "markets".to_string(),
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
                description: "".to_string(),
                extra_snippets: vec![],
                category: "crypto".to_string(),
                published_at: 1709620000,
                fetched_at: "2024-03-05 12:00:00".to_string(),
            },
        ];

        let result = print_json(&entries);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_empty_cache_json() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        // JSON mode with empty cache should return Ok (exit 0), not error
        let result = run(&backend, None, None, None, 20, false, true);
        assert!(result.is_ok(), "JSON mode should not fail on empty cache");
    }

    #[test]
    fn test_run_empty_cache_text() {
        let conn = crate::db::open_in_memory();
        let backend = to_backend(conn);

        let result = run(&backend, None, None, None, 20, false, false);
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
        let result = run(&backend, None, None, None, 20, false, true);
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
        let result = run(&backend, None, None, None, 20, false, false);
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
        let result = run(&backend, None, None, None, 20, true, true);
        assert!(
            result.is_ok(),
            "JSON mode with sentiment should succeed with entries"
        );
    }
}
