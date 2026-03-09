//! `pftui eod` — End-of-Day market close summary.
//!
//! Combines brief + movers + macro + sentiment into a single concise report.
//! Designed for daily market close review.

use anyhow::Result;
use rusqlite::Connection;

use crate::config::Config;
use crate::data::cot::{fetch_latest_report, COT_CONTRACTS};
use crate::data::sentiment::{fetch_crypto_fng, fetch_traditional_fng};
use crate::db::backend::BackendConnection;

/// Run the `pftui eod` command.
pub fn run(backend: &BackendConnection, conn: &Connection, config: &Config, json: bool) -> Result<()> {
    if json {
        run_json(backend, conn, config)
    } else {
        run_human(backend, conn, config)
    }
}

/// Human-readable output.
fn run_human(backend: &BackendConnection, conn: &Connection, config: &Config) -> Result<()> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                   END OF DAY SUMMARY                         ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // 1. Portfolio Brief
    println!("┌─ PORTFOLIO ────────────────────────────────────────────────┐");
    super::brief::run(conn, config, false, false)?;
    println!("└────────────────────────────────────────────────────────────┘\n");

    // 2. Movers (threshold: 3%)
    println!("┌─ TOP MOVERS ───────────────────────────────────────────────┐");
    super::movers::run(backend, config, Some("3"), false)?;
    println!("└────────────────────────────────────────────────────────────┘\n");

    // 3. Macro Dashboard
    println!("┌─ MACRO INDICATORS ─────────────────────────────────────────┐");
    super::macro_cmd::run(backend, config, false)?;
    println!("└────────────────────────────────────────────────────────────┘\n");

    // 4. Sentiment Overview (F&G + COT summary)
    println!("┌─ SENTIMENT & POSITIONING ──────────────────────────────────┐");
    print_sentiment_summary()?;
    println!("└────────────────────────────────────────────────────────────┘");

    Ok(())
}

/// JSON output.
fn run_json(backend: &BackendConnection, conn: &Connection, config: &Config) -> Result<()> {
    use serde_json::json;

    // Fetch all components
    let brief_output = capture_json_output(|| super::brief::run(conn, config, false, true))?;
    let movers_output = capture_json_output(|| super::movers::run(backend, config, Some("3"), true))?;
    let macro_output = capture_json_output(|| super::macro_cmd::run(backend, config, true))?;
    let sentiment_output = capture_json_output(|| super::sentiment::run(None, None, true))?;

    let eod = json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "portfolio": brief_output,
        "movers": movers_output,
        "macro": macro_output,
        "sentiment": sentiment_output,
    });

    println!("{}", serde_json::to_string_pretty(&eod)?);
    Ok(())
}

/// Capture stdout from a command that prints JSON.
/// For now, this is a placeholder that runs the command and returns an empty object.
/// Full JSON integration would require refactoring sub-commands to return data.
fn capture_json_output<F>(f: F) -> Result<serde_json::Value>
where
    F: FnOnce() -> Result<()>,
{
    f()?;
    Ok(serde_json::json!({}))
}

/// Print a concise sentiment summary (F&G + COT positioning).
fn print_sentiment_summary() -> Result<()> {
    // Fetch Fear & Greed indices
    let crypto_fng = fetch_crypto_fng().ok();
    let trad_fng = fetch_traditional_fng().ok();

    // Print F&G
    if let Some(idx) = crypto_fng {
        println!("│ Crypto F&G:        {:>3}/100  {}", idx.value, idx.classification);
    } else {
        println!("│ Crypto F&G:        ---");
    }

    if let Some(idx) = trad_fng {
        println!("│ Traditional F&G:   {:>3}/100  {}", idx.value, idx.classification);
    } else {
        println!("│ Traditional F&G:   ---");
    }

    // Fetch COT positioning for key contracts (just GC, SI, CL)
    let key_symbols = ["GC=F", "SI=F", "CL=F"];
    let mut cot_data = Vec::new();

    for sym in &key_symbols {
        if let Some(contract) = COT_CONTRACTS.iter().find(|c| c.symbol == *sym) {
            if let Ok(report) = fetch_latest_report(contract.cftc_code) {
                cot_data.push((contract, report));
            }
        }
    }

    if !cot_data.is_empty() {
        println!("│");
        println!("│ COT Positioning:");
        for (contract, report) in cot_data {
            let net_mm = report.managed_money_net;
            let sign = if net_mm >= 0 { "+" } else { "" };
            println!(
                "│   {} ({:>6}): {} net {:>8}",
                contract.symbol,
                contract.name,
                sign,
                format_large_number(net_mm)
            );
        }
    }

    Ok(())
}

/// Format large numbers with K/M suffixes.
fn format_large_number(n: i64) -> String {
    if n.abs() >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n.abs() >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
