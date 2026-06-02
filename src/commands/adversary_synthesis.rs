//! Handlers for `pftui analytics adversary synthesis {add,show,fragility-rank}`.
//!
//! These CLI surfaces wrap the `adversary_synthesis_views` SQLite table, the
//! per-asset/per-run "case against the convergence" written by the
//! `analyst-adversary` pseudo-layer (`agents/routines/adversary-analyst.md`).
//!
//! Sister surface to the write-time per-prediction adversary under
//! `pftui journal prediction adversary` (see `commands::predict`). These two
//! adversary surfaces are intentionally separate (different cardinality,
//! different consumer) and use distinct tables (`adversary_views` for
//! write-time, `adversary_synthesis_views` for synthesis-time).

use anyhow::{anyhow, Result};
use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;

use crate::db::adversary_synthesis_views;
use crate::db::backend::BackendConnection;

fn require_sqlite(backend: &BackendConnection) -> Result<&rusqlite::Connection> {
    backend.sqlite_native().ok_or_else(|| {
        anyhow!("analytics adversary synthesis commands require the SQLite backend")
    })
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Parse a `--since` argument. Accepted forms:
///
/// - `YYYY-MM-DD` absolute date
/// - `Nd` / `Nw` / `Nm` relative windows (months = 30 days)
///
/// Returns an ISO-8601 datetime string at the start of the resulting day
/// so it can be compared against `recorded_at TEXT` columns that store
/// `datetime('now')` style timestamps.
pub(crate) fn parse_since(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if let Ok(d) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Ok(format!("{}T00:00:00", d.format("%Y-%m-%d")));
    }
    let split_at = trimmed
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (num_part, unit) = trimmed.split_at(split_at);
    let n: i64 = num_part.parse().map_err(|_| {
        anyhow!(
            "invalid --since: expected NNd/NNw/NNm or YYYY-MM-DD, got '{}'",
            input
        )
    })?;
    let days = match unit {
        "d" | "" => n,
        "w" => n * 7,
        "m" => n * 30,
        other => anyhow::bail!("unknown --since unit '{}' (use d/w/m or YYYY-MM-DD)", other),
    };
    let date = Utc::now().date_naive() - Duration::days(days);
    Ok(format!("{}T00:00:00", date.format("%Y-%m-%d")))
}

/// Validate caller-supplied JSON array string; reject inputs that aren't an
/// array so we don't silently persist garbage that breaks downstream
/// renderers expecting `Vec<String>`.
fn validate_json_array(label: &str, raw: &str) -> Result<String> {
    let v: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| anyhow!("--{} must be a JSON array, got invalid JSON: {}", label, e))?;
    if !v.is_array() {
        anyhow::bail!("--{} must be a JSON array, e.g. '[\"a\",\"b\"]'", label);
    }
    // re-serialise to normalise whitespace
    Ok(serde_json::to_string(&v)?)
}

#[allow(clippy::too_many_arguments)]
pub fn synthesis_add(
    backend: &BackendConnection,
    asset: &str,
    convergence: &str,
    counter: &str,
    evidence: &str,
    falsification: &str,
    fragility: i64,
    recorded_at: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let evidence_norm = validate_json_array("evidence", evidence)?;
    let falsification_norm = validate_json_array("falsification", falsification)?;
    let id = adversary_synthesis_views::insert(
        conn,
        asset,
        convergence,
        counter,
        &evidence_norm,
        &falsification_norm,
        fragility,
        recorded_at,
    )?;
    let row = adversary_synthesis_views::get(conn, id)?
        .ok_or_else(|| anyhow!("inserted adversary_synthesis_views row {} not re-readable", id))?;
    if json {
        return print_json(&row);
    }
    println!(
        "Recorded synthesis-time adversary view #{} for {} (fragility={})",
        row.id, row.asset, row.fragility_score
    );
    println!("  convergence: {}", row.current_convergence_summary);
    println!("  counter:     {}", row.counter_case_summary);
    println!("  recorded_at: {}", row.recorded_at);
    if row.fragility_score >= 3 {
        println!(
            "  note: fragility_score={} >= 3 — synthesis MUST address this counter-case",
            row.fragility_score
        );
    }
    Ok(())
}

pub fn synthesis_show(
    backend: &BackendConnection,
    asset: Option<&str>,
    since: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let since_resolved = since.map(parse_since).transpose()?;
    let rows = adversary_synthesis_views::list(conn, asset, since_resolved.as_deref())?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No synthesis-time adversary views match.");
        return Ok(());
    }
    for r in &rows {
        println!(
            "#{:<4} {:<6} fragility={} recorded_at={}",
            r.id, r.asset, r.fragility_score, r.recorded_at
        );
        println!("  convergence: {}", r.current_convergence_summary);
        println!("  counter:     {}", r.counter_case_summary);
    }
    Ok(())
}

pub fn synthesis_fragility_rank(
    backend: &BackendConnection,
    since: Option<&str>,
    json: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let since_resolved = since.map(parse_since).transpose()?;
    let rows = adversary_synthesis_views::fragility_rank(conn, since_resolved.as_deref())?;
    if json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("No synthesis-time adversary views in window.");
        return Ok(());
    }
    for r in &rows {
        println!(
            "{:<6} max_fragility={} latest={}",
            r.asset, r.max_fragility_score, r.latest_recorded_at
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::adversary_synthesis_views::ensure_table;
    use rusqlite::Connection;

    fn fresh_backend() -> BackendConnection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        ensure_table(&conn).expect("ensure_table");
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn parse_since_handles_iso_and_relative() {
        let iso = parse_since("2026-05-01").unwrap();
        assert!(iso.starts_with("2026-05-01T"));
        let d = parse_since("7d").unwrap();
        assert!(d.contains('T'));
        assert!(parse_since("garbage").is_err());
        assert!(parse_since("5x").is_err());
    }

    #[test]
    fn add_then_show_round_trip_via_json_helpers() {
        let backend = fresh_backend();
        synthesis_add(
            &backend,
            "BTC",
            "All four layers expect $100k by Q3.",
            "Cycle top is closer than convergence implies.",
            "[\"realized cap stalling\"]",
            "[\"BTC < $65k for 5 sessions\"]",
            4,
            Some("2026-06-02T18:00:00Z"),
            true,
        )
        .unwrap();

        let conn = backend.sqlite_native().unwrap();
        let rows = adversary_synthesis_views::list(conn, Some("BTC"), None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].asset, "BTC");
        assert_eq!(rows[0].fragility_score, 4);
        assert!(rows[0]
            .counter_case_summary
            .contains("Cycle top"));
        // normalised JSON should still contain the array element.
        assert!(rows[0].counter_case_evidence_points.contains("realized"));
    }

    #[test]
    fn add_rejects_non_array_evidence_json() {
        let backend = fresh_backend();
        let err = synthesis_add(
            &backend,
            "BTC",
            "c",
            "k",
            "{\"oops\":true}",
            "[]",
            3,
            None,
            true,
        );
        assert!(err.is_err());
        let err2 = synthesis_add(
            &backend,
            "BTC",
            "c",
            "k",
            "[",
            "[]",
            3,
            None,
            true,
        );
        assert!(err2.is_err());
    }

    #[test]
    fn fragility_rank_orders_correctly() {
        let backend = fresh_backend();
        // Use the DB helper directly to seed; the wrapper around
        // fragility_rank is what we want to exercise here.
        let conn = backend.sqlite_native().unwrap();
        adversary_synthesis_views::insert(
            conn, "BTC", "c", "k", "[]", "[]", 2,
            Some("2026-06-01T00:00:00Z"),
        )
        .unwrap();
        adversary_synthesis_views::insert(
            conn, "BTC", "c", "k", "[]", "[]", 5,
            Some("2026-06-02T00:00:00Z"),
        )
        .unwrap();
        adversary_synthesis_views::insert(
            conn, "GLD", "c", "k", "[]", "[]", 4,
            Some("2026-06-02T00:00:00Z"),
        )
        .unwrap();
        adversary_synthesis_views::insert(
            conn, "AAA", "c", "k", "[]", "[]", 5,
            Some("2026-06-02T00:00:00Z"),
        )
        .unwrap();
        let rows = adversary_synthesis_views::fragility_rank(conn, None).unwrap();
        assert_eq!(rows.len(), 3);
        // AAA and BTC tie at 5; AAA < BTC alphabetically.
        assert_eq!(rows[0].asset, "AAA");
        assert_eq!(rows[1].asset, "BTC");
        assert_eq!(rows[2].asset, "GLD");
    }
}
