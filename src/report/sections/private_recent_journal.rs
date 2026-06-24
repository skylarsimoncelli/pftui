#![allow(dead_code)]

use anyhow::Result;

use crate::report::build::daily::BuildContext;

pub fn render_private_recent_journal(ctx: &BuildContext) -> Result<String> {
    if ctx.private_recent_journal.is_empty() {
        return Ok(super::suppressed("no recent journal entries"));
    }

    let mut out = String::from("## Recent Journal\n\n");
    out.push_str("| Time | Author | Symbol | Tags | Status | Note |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for row in &ctx.private_recent_journal {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            clean(&short_time(&row.timestamp)),
            clean(&row.author),
            clean(row.symbol.as_deref().unwrap_or("n/a")),
            clean(row.tag.as_deref().unwrap_or("n/a")),
            clean(&row.status),
            clean(&truncate(&row.content, 180)),
        ));
    }
    Ok(out.trim_end().to_string())
}

fn short_time(value: &str) -> String {
    value.chars().take(16).collect()
}

fn truncate(value: &str, max: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
        out.push_str("...");
        out
    }
}

fn clean(value: &str) -> String {
    value
        .replace('|', "/")
        .replace('\n', " ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::build::daily::PrivateRecentJournalRow;

    #[test]
    fn renders_recent_journal_rows() {
        let ctx = BuildContext {
            private_recent_journal: vec![PrivateRecentJournalRow {
                id: 1,
                timestamp: "2026-06-24T10:00:00Z".to_string(),
                author: "skylar".to_string(),
                symbol: Some("BTC".to_string()),
                tag: Some("cycle".to_string()),
                status: "open".to_string(),
                content: "Watch monthly cycle checks.".to_string(),
            }],
            ..BuildContext::default()
        };
        let out = render_private_recent_journal(&ctx).unwrap();
        assert!(out.contains("## Recent Journal"));
        assert!(out.contains("Watch monthly cycle checks."));
    }
}
