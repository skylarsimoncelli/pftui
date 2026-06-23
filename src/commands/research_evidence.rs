//! `pftui research evidence {add,list}` — the research-evidence ledger CLI.
//!
//! Writer (`add`) appends one captured source/finding row to the
//! `research_evidence` L3 ledger. Reader (`list`) returns the filtered,
//! newest-first record. Both emit `--json`.

use anyhow::Result;
use rust_decimal::Decimal;

use crate::db::backend::BackendConnection;
use crate::db::research_evidence::{self, EvidenceFilter, EvidenceRow};

fn require_sqlite(backend: &BackendConnection) -> Result<&rusqlite::Connection> {
    backend
        .sqlite_native()
        .ok_or_else(|| anyhow::anyhow!("research evidence requires the SQLite backend"))
}

fn today() -> String {
    chrono::Utc::now().date_naive().format("%Y-%m-%d").to_string()
}

/// `pftui research evidence add ...`
#[allow(clippy::too_many_arguments)]
pub fn run_add(
    backend: &BackendConnection,
    layer: &str,
    asset: Option<&str>,
    claim: &str,
    source: &str,
    url: Option<&str>,
    source_date: Option<&str>,
    finding: &str,
    stance: Option<&str>,
    confidence: Option<&str>,
    run_date: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let run_date = run_date.map(|s| s.to_string()).unwrap_or_else(today);
    let row = research_evidence::add(
        conn,
        &run_date,
        layer,
        asset,
        claim,
        source,
        url,
        source_date,
        finding,
        stance,
        confidence,
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&row)?);
        return Ok(());
    }
    println!(
        "Recorded evidence #{} — [{}] {} on {}",
        row.id,
        row.layer,
        row.source_name,
        row.asset.as_deref().unwrap_or("macro-wide")
    );
    println!("  claim:   {}", row.claim);
    println!("  finding: {}", row.finding);
    if let Some(url) = &row.source_url {
        println!("  url:     {url}");
    }
    Ok(())
}

/// `pftui research evidence list ...`
pub fn run_list(
    backend: &BackendConnection,
    asset: Option<&str>,
    layer: Option<&str>,
    since: Option<&str>,
    source: Option<&str>,
    limit: Option<i64>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let filter = EvidenceFilter {
        asset,
        layer,
        since,
        source,
        limit,
    };
    let rows = research_evidence::list(conn, &filter)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No research evidence recorded for that filter.");
        return Ok(());
    }
    println!("Research evidence ({} row(s), newest first):", rows.len());
    for r in &rows {
        render_row(r);
    }
    Ok(())
}

fn render_row(r: &EvidenceRow) {
    let conf = r
        .confidence
        .as_deref()
        .and_then(|c| Decimal::from_str_exact(c).ok())
        .map(|d| format!(" conf {d}"))
        .unwrap_or_default();
    let stance = r
        .stance
        .as_deref()
        .map(|s| format!(" [{s}]"))
        .unwrap_or_default();
    println!(
        "  #{} {} [{}] {} — {}{}{}",
        r.id,
        r.run_date,
        r.layer,
        r.asset.as_deref().unwrap_or("macro-wide"),
        r.source_name,
        stance,
        conf
    );
    println!("      claim:   {}", r.claim);
    println!("      finding: {}", r.finding);
    if let Some(url) = &r.source_url {
        let dated = r
            .source_date
            .as_deref()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        println!("      source:  {url}{dated}");
    } else if let Some(d) = &r.source_date {
        println!("      source:  ({d})");
    }
}
