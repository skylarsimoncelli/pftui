use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::journal::{self, NewJournalEntry};

#[derive(Debug, Default, Serialize, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub parsed: usize,
    pub inserted: usize,
    pub skipped: usize,
}

#[derive(Debug, Default, Clone)]
struct InlineMeta {
    timestamp: Option<String>,
    tag: Option<String>,
    symbol: Option<String>,
    conviction: Option<String>,
    status: Option<String>,
}

pub fn run(
    conn: &Connection,
    path: &str,
    dry_run: bool,
    default_tag: Option<&str>,
    default_status: &str,
    json_output: bool,
) -> Result<()> {
    let markdown = std::fs::read_to_string(path)?;
    let report = migrate_from_markdown(conn, &markdown, dry_run, default_tag, default_status)?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        if dry_run {
            println!(
                "Dry run for '{}': parsed {}, would insert {}, skipped {}",
                path, report.parsed, report.inserted, report.skipped
            );
        } else {
            println!(
                "Migration from '{}': parsed {}, inserted {}, skipped {}",
                path, report.parsed, report.inserted, report.skipped
            );
        }
    }
    Ok(())
}

fn migrate_from_markdown(
    conn: &Connection,
    markdown: &str,
    dry_run: bool,
    default_tag: Option<&str>,
    default_status: &str,
) -> Result<MigrationReport> {
    let parsed = parse_markdown_entries(markdown, default_tag, default_status);
    let mut report = MigrationReport {
        parsed: parsed.len(),
        ..MigrationReport::default()
    };

    for entry in parsed {
        if entry_exists(conn, &entry.timestamp, &entry.content)? {
            report.skipped += 1;
            continue;
        }
        if !dry_run {
            let _ = journal::add_entry(conn, &entry)?;
        }
        report.inserted += 1;
    }

    Ok(report)
}

fn entry_exists(conn: &Connection, timestamp: &str, content: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM journal WHERE timestamp = ?1 AND content = ?2",
        rusqlite::params![timestamp, content],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn parse_markdown_entries(
    markdown: &str,
    default_tag: Option<&str>,
    default_status: &str,
) -> Vec<NewJournalEntry> {
    let mut entries = Vec::new();
    let mut current_timestamp: Option<String> = None;
    let mut current_tag: Option<String> = None;
    let mut current_status = normalize_status(default_status).to_string();

    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(heading) = parse_heading(trimmed) {
            if let Some(ts) = parse_timestamp(&heading) {
                current_timestamp = Some(ts);
            }
            if let Some(tag) = infer_tag_from_heading(&heading) {
                current_tag = Some(tag);
            }
            if let Some(status) = infer_status(&heading) {
                current_status = status.to_string();
            }
            continue;
        }

        let Some(raw_content) = parse_list_item(trimmed) else {
            continue;
        };

        let (meta, cleaned) = extract_inline_meta(raw_content);
        let content = cleaned.trim().to_string();
        if content.is_empty() {
            continue;
        }

        let status = meta
            .status
            .as_deref()
            .map(normalize_status)
            .unwrap_or(&current_status)
            .to_string();
        let timestamp = meta
            .timestamp
            .or_else(|| current_timestamp.clone())
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let symbol = meta.symbol.or_else(|| infer_symbol_from_content(&content));
        let tag = meta
            .tag
            .or_else(|| current_tag.clone())
            .or_else(|| default_tag.map(|t| t.to_string()));

        entries.push(NewJournalEntry {
            timestamp,
            content,
            tag,
            symbol,
            conviction: meta.conviction,
            status,
        });
    }

    entries
}

fn parse_heading(line: &str) -> Option<String> {
    if !line.starts_with('#') {
        return None;
    }
    Some(line.trim_start_matches('#').trim().to_string())
}

fn parse_list_item(line: &str) -> Option<&str> {
    if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
        return Some(line[2..].trim());
    }
    let mut seen_digit = false;
    for (idx, ch) in line.char_indices() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            continue;
        }
        if seen_digit && ch == '.' {
            let next = line.get(idx + 1..idx + 2)?;
            if next == " " {
                return Some(line[idx + 2..].trim());
            }
        }
        break;
    }
    None
}

fn parse_timestamp(raw: &str) -> Option<String> {
    let text = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
        return Some(dt.to_rfc3339());
    }
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        let dt = date.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).to_rfc3339());
    }
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y/%m/%d") {
        let dt = date.and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc).to_rfc3339());
    }
    let mut token = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '-' || ch == '/' {
            token.push(ch);
        } else if !token.is_empty() {
            break;
        }
    }
    if token.is_empty() {
        return None;
    }
    parse_timestamp(&token)
}

fn infer_tag_from_heading(heading: &str) -> Option<String> {
    let h = heading.to_lowercase();
    let known = [
        "trade",
        "thesis",
        "prediction",
        "reflection",
        "alert",
        "lesson",
        "call",
        "move",
    ];
    known
        .iter()
        .find(|k| h.contains(**k))
        .map(|s| s.to_string())
}

fn normalize_status(status: &str) -> &str {
    match status.trim().to_lowercase().as_str() {
        "validated" => "validated",
        "invalidated" => "invalidated",
        "closed" => "closed",
        _ => "open",
    }
}

fn infer_status(text: &str) -> Option<&'static str> {
    let l = text.to_lowercase();
    if l.contains("validated") {
        Some("validated")
    } else if l.contains("invalidated") {
        Some("invalidated")
    } else if l.contains("closed") {
        Some("closed")
    } else if l.contains("open") {
        Some("open")
    } else {
        None
    }
}

fn extract_inline_meta(content: &str) -> (InlineMeta, String) {
    let mut out = InlineMeta::default();
    let mut cleaned_parts = Vec::new();
    for part in content.split_whitespace() {
        if part.starts_with('[') && part.ends_with(']') && part.len() > 2 {
            let raw = &part[1..part.len() - 1];
            let (k, v) = raw.split_once(':').or_else(|| raw.split_once('=')).unwrap_or(("", ""));
            let key = k.trim().to_lowercase();
            let val = v.trim();
            if val.is_empty() {
                cleaned_parts.push(part.to_string());
                continue;
            }
            match key.as_str() {
                "tag" => out.tag = Some(val.to_lowercase()),
                "symbol" => out.symbol = Some(val.to_uppercase()),
                "status" => out.status = Some(val.to_lowercase()),
                "conviction" => out.conviction = Some(val.to_lowercase()),
                "date" | "timestamp" | "time" => out.timestamp = parse_timestamp(val),
                _ => cleaned_parts.push(part.to_string()),
            }
            continue;
        }
        cleaned_parts.push(part.to_string());
    }
    let cleaned = cleaned_parts.join(" ");
    (out, cleaned)
}

fn infer_symbol_from_content(content: &str) -> Option<String> {
    for token in content.split_whitespace() {
        if let Some(sym) = token.strip_prefix('$') {
            let candidate = normalize_symbol_token(sym);
            if !candidate.is_empty() {
                return Some(candidate);
            }
        }
    }
    for token in content.split_whitespace() {
        let candidate = normalize_symbol_token(token);
        if candidate.contains('=') || candidate.contains('-') {
            return Some(candidate);
        }
    }
    None
}

fn normalize_symbol_token(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '=' || *c == '-' || *c == '.')
        .collect::<String>()
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn parses_heading_dates_and_bullets() {
        let md = r#"
## 2026-03-05
- Added gold position [tag:trade] [symbol:GC=F]
- Thesis update on BRICS
"#;
        let entries = parse_markdown_entries(md, None, "open");
        assert_eq!(entries.len(), 2);
        assert!(entries[0].timestamp.starts_with("2026-03-05"));
        assert_eq!(entries[0].tag.as_deref(), Some("trade"));
        assert_eq!(entries[0].symbol.as_deref(), Some("GC=F"));
        assert_eq!(entries[1].status, "open");
    }

    #[test]
    fn parses_inline_metadata_and_status() {
        let md = r#"
### Open Calls
- Buy the dip [status:validated] [conviction:high] [date:2026-03-01]
"#;
        let entries = parse_markdown_entries(md, Some("call"), "open");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "validated");
        assert_eq!(entries[0].conviction.as_deref(), Some("high"));
        assert_eq!(entries[0].tag.as_deref(), Some("call"));
        assert!(entries[0].timestamp.starts_with("2026-03-01"));
    }

    #[test]
    fn migration_is_idempotent_via_dedupe() {
        let conn = open_in_memory();
        let md = r#"
## 2026-03-05
- Added BTC starter [tag:trade] [symbol:BTC]
"#;
        let first = migrate_from_markdown(&conn, md, false, None, "open").unwrap();
        assert_eq!(first.parsed, 1);
        assert_eq!(first.inserted, 1);
        assert_eq!(first.skipped, 0);

        let second = migrate_from_markdown(&conn, md, false, None, "open").unwrap();
        assert_eq!(second.parsed, 1);
        assert_eq!(second.inserted, 0);
        assert_eq!(second.skipped, 1);
    }
}
