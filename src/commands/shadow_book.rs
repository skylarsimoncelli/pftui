//! `pftui research shadowbook` — render the shadow-book benchmark.
//!
//! All computation lives in `research::shadow_book`; this module is
//! presentation only (text table + `--json`).

use anyhow::Result;
use rusqlite::Connection;
use rust_decimal::Decimal;

use crate::db::backend::BackendConnection;
use crate::research::shadow_book::{self, ShadowBookReport, ACCRUING_DAYS, POLICY_VERSION};

fn sqlite(backend: &BackendConnection) -> Result<&Connection> {
    backend
        .sqlite_native()
        .ok_or_else(|| anyhow::anyhow!("research shadowbook requires the SQLite backend"))
}

/// One-line summary for embedding in other surfaces (`analytics epistemics
/// show`). Returns `None` when the ledger has no action rows yet.
pub fn summary_line(conn: &Connection, today: &str) -> Option<String> {
    let report = shadow_book::compute(conn, today).ok()??;
    if report.days >= 30 {
        Some(format!("Shadow book: {}", report.verdict))
    } else {
        Some(format!(
            "Shadow book: accruing — {} day(s) of ledger history since {} (summary line matures at 30d)",
            report.days, report.inception
        ))
    }
}

pub fn run(backend: &BackendConnection, json: bool) -> Result<()> {
    let conn = sqlite(backend)?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let report = shadow_book::compute(conn, &today)?;

    let Some(report) = report else {
        if json {
            println!("{}", serde_json::json!({ "report": null }));
        } else {
            println!(
                "No recommendations-ledger action rows yet — nothing to benchmark.\n\
                 Record decisions with `pftui analytics recommendations record` first."
            );
        }
        return Ok(());
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "report": report }))?
        );
    } else {
        print_text(&report);
    }
    Ok(())
}

fn fmt_money(v: Decimal) -> String {
    v.round_dp(2).to_string()
}

fn print_text(r: &ShadowBookReport) {
    if r.accruing {
        println!(
            "⏳ BENCHMARK ACCRUING — {} day(s) of ledger history (< {ACCRUING_DAYS}d). All figures provisional.\n",
            r.days
        );
    }
    println!("Shadow book — {POLICY_VERSION}");
    println!(
        "  Mechanical policy: add → +1.0pp NAV cash→symbol at the row's entry price;\n\
         \x20 trim → −1.0pp symbol→cash (capped at held value); wait/hold/avoid → no trade;\n\
         \x20 same-day rows in id order; adds skipped when cash < 1pp of NAV."
    );
    println!();
    println!("  Inception: {} (first ledger row)   As of: {}", r.inception, r.as_of);
    println!();
    println!("  {:<8} {:>14} {:>10}", "book", "NAV", "return");
    println!("  {}", "-".repeat(36));
    println!(
        "  {:<8} {:>14} {:>9.2}%",
        "shadow",
        fmt_money(r.shadow_nav),
        r.shadow_return_pct
    );
    println!(
        "  {:<8} {:>14} {:>9.2}%",
        "actual",
        fmt_money(r.actual_nav),
        r.actual_return_pct
    );
    println!(
        "  {:<8} {:>14} {:>9.2}%",
        "hold",
        fmt_money(r.hold_nav),
        r.hold_return_pct
    );
    println!();
    println!("  {}", r.verdict);

    if !r.executed.is_empty() {
        println!("\n  Executed trades (P&L vs not having done it, cash flat):");
        println!(
            "  {:>5}  {:<10}  {:<10}  {:<6}  {:>12}  {:>12}  {:>12}",
            "rec", "date", "symbol", "action", "entry", "value", "pnl vs skip"
        );
        for t in &r.executed {
            println!(
                "  {:>5}  {:<10}  {:<10}  {:<6}  {:>12}  {:>12}  {:>12}",
                t.rec_id,
                t.date,
                t.symbol,
                t.action,
                fmt_money(t.entry_price),
                fmt_money(t.trade_value),
                t.pnl_vs_skip
                    .map(|p| format!("{:+}", p.round_dp(2)))
                    .unwrap_or_else(|| "—".to_string()),
            );
        }
    }

    if r.waits > 0 {
        println!("\n  No-trade rows (wait/hold/avoid): {}", r.waits);
    }

    if !r.ledger_rows.is_empty() {
        const MAX_ROWS: usize = 20;
        let start = r.ledger_rows.len().saturating_sub(MAX_ROWS);
        if start > 0 {
            println!(
                "\n  Ledger (showing last {} of {} rows):",
                MAX_ROWS,
                r.ledger_rows.len()
            );
        } else {
            println!("\n  Ledger ({} rows):", r.ledger_rows.len());
        }
        println!(
            "  {:>5}  {:<10}  {:<10}  {:<6}  disposition",
            "rec", "date", "symbol", "action"
        );
        for row in &r.ledger_rows[start..] {
            println!(
                "  {:>5}  {:<10}  {:<10}  {:<6}  {}",
                row.rec_id, row.date, row.symbol, row.action, row.disposition
            );
        }
    }

    if !r.skipped.is_empty() {
        println!("\n  Skipped trades:");
        for s in &r.skipped {
            println!(
                "  ⚠ rec {} {} {} {} — {}",
                s.rec_id, s.date, s.symbol, s.action, s.reason
            );
        }
    }

    if !r.warnings.is_empty() {
        println!("\n  Warnings:");
        for w in &r.warnings {
            println!("  ⚠ {w}");
        }
    }
    println!(
        "\n  Note: ACTUAL return includes external flows (deposits/withdrawals are not\n\
         \x20 adjusted out in policy v1). Computed on demand — no shadow positions are stored."
    );
}
