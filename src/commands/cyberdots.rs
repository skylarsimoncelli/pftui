use anyhow::{bail, Result};
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::cyberdots::{self, AddSignal, CyberdotsSignal, ListFilter};

/// Normalize a `--since` token. Accepts either a YYYY-MM-DD date or a
/// short window like `30d`, `7d`, `12h`, `90d`. Returns a SQLite datetime
/// comparison string.
fn normalize_since(token: &str) -> Result<String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        bail!("--since cannot be empty");
    }
    // Plain date — pass through as-is; SQLite TEXT comparison is fine since
    // recorded_at is `YYYY-MM-DD HH:MM:SS` lexicographic-ordered.
    if chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d").is_ok() {
        return Ok(trimmed.to_string());
    }
    // Window form: <number><h|d|w>
    let (num_part, unit) = trimmed.split_at(
        trimmed
            .find(|c: char| !c.is_ascii_digit())
            .ok_or_else(|| anyhow::anyhow!("Invalid --since '{}'; expected '30d' or YYYY-MM-DD", trimmed))?,
    );
    let n: i64 = num_part
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid --since '{}'; expected '30d' or YYYY-MM-DD", trimmed))?;
    let secs = match unit {
        "h" => n * 3600,
        "d" => n * 86400,
        "w" => n * 86400 * 7,
        _ => bail!("Invalid --since unit '{unit}'; use h, d, or w (or YYYY-MM-DD)"),
    };
    let cutoff = chrono::Utc::now() - chrono::Duration::seconds(secs);
    Ok(cutoff.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn require_sqlite(backend: &BackendConnection) -> Result<&rusqlite::Connection> {
    backend.sqlite_native().ok_or_else(|| {
        anyhow::anyhow!(
            "`pftui portfolio cyberdots` currently requires the SQLite backend; \
             postgres support is forthcoming"
        )
    })
}

fn signal_to_json(sig: &CyberdotsSignal) -> serde_json::Value {
    json!({
        "id": sig.id,
        "symbol": sig.symbol,
        "timeframe": sig.timeframe,
        "recorded_at": sig.recorded_at,
        "dot_state": sig.dot_state,
        "trackline_position": sig.trackline_position,
        "flip_from_prior": sig.flip_from_prior,
        "source": sig.source,
        "notes": sig.notes,
        "related_transaction_id": sig.related_transaction_id,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn run_add(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: &str,
    dot_state: &str,
    trackline_position: &str,
    source: Option<&str>,
    notes: Option<&str>,
    related_transaction_id: Option<i64>,
    json_out: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let symbol = symbol.trim();
    if symbol.is_empty() {
        bail!("--symbol cannot be empty");
    }
    let src = source.unwrap_or("skylar-manual");
    let args = AddSignal {
        symbol,
        timeframe,
        dot_state,
        trackline_position,
        source: src,
        notes,
        related_transaction_id,
    };
    let (id, flip) = cyberdots::add_signal(conn, &args)?;

    if json_out {
        let out = json!({
            "id": id,
            "symbol": symbol,
            "timeframe": timeframe,
            "dot_state": dot_state,
            "trackline_position": trackline_position,
            "flip_from_prior": flip,
            "source": src,
            "notes": notes,
            "related_transaction_id": related_transaction_id,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "Recorded CyberDots signal #{} for {} {} — {}/{} (flip: {})",
            id,
            symbol,
            timeframe,
            dot_state,
            trackline_position,
            flip.as_deref().unwrap_or("baseline")
        );
    }
    Ok(())
}

pub fn run_flip(
    backend: &BackendConnection,
    symbol: &str,
    timeframe: &str,
    new_dot_state: &str,
    notes: Option<&str>,
    json_out: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let symbol = symbol.trim();
    if symbol.is_empty() {
        bail!("symbol cannot be empty");
    }
    cyberdots::validate_timeframe(timeframe)?;
    cyberdots::validate_dot_state(new_dot_state)?;

    let prior = cyberdots::latest_for(conn, symbol, timeframe)?;
    let trackline = prior
        .as_ref()
        .map(|s| s.trackline_position.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No prior signal for {} {}; use `pftui portfolio cyberdots add` for the first row \
                 so trackline position is recorded explicitly",
                symbol,
                timeframe
            )
        })?;

    let owned_notes = notes.map(str::to_string);
    let final_notes = owned_notes.as_deref().unwrap_or("flip detected");
    let args = AddSignal {
        symbol,
        timeframe,
        dot_state: new_dot_state,
        trackline_position: &trackline,
        source: "skylar-manual",
        notes: Some(final_notes),
        related_transaction_id: None,
    };
    let (id, flip) = cyberdots::add_signal(conn, &args)?;

    if json_out {
        let out = json!({
            "id": id,
            "symbol": symbol,
            "timeframe": timeframe,
            "dot_state": new_dot_state,
            "trackline_position": trackline,
            "flip_from_prior": flip,
            "source": "skylar-manual",
            "notes": final_notes,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "Flip recorded #{} for {} {} — now {} (trackline {}, prior flip {})",
            id,
            symbol,
            timeframe,
            new_dot_state,
            trackline,
            flip.as_deref().unwrap_or("baseline")
        );
    }
    Ok(())
}

pub fn run_list(
    backend: &BackendConnection,
    symbol: Option<&str>,
    timeframe: Option<&str>,
    since: Option<&str>,
    json_out: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let owned_since = since.map(normalize_since).transpose()?;
    let filter = ListFilter {
        symbol,
        timeframe,
        since: owned_since.as_deref(),
        flips_only: false,
        limit: None,
    };
    let rows = cyberdots::list_signals(conn, &filter)?;
    if json_out {
        let arr: Vec<_> = rows.iter().map(signal_to_json).collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No CyberDots signals recorded.");
        return Ok(());
    }
    print_table(&rows);
    Ok(())
}

pub fn run_flips(
    backend: &BackendConnection,
    symbol: Option<&str>,
    since: Option<&str>,
    json_out: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let owned_since = since.map(normalize_since).transpose()?;
    let filter = ListFilter {
        symbol,
        timeframe: None,
        since: owned_since.as_deref(),
        flips_only: true,
        limit: None,
    };
    let rows = cyberdots::list_signals(conn, &filter)?;
    if json_out {
        let arr: Vec<_> = rows.iter().map(signal_to_json).collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No CyberDots flips recorded in window.");
        return Ok(());
    }
    print_table(&rows);
    Ok(())
}

pub fn run_current(
    backend: &BackendConnection,
    symbol: Option<&str>,
    json_out: bool,
) -> Result<()> {
    let conn = require_sqlite(backend)?;
    let rows = cyberdots::current_signals(conn, symbol)?;
    if json_out {
        let arr: Vec<_> = rows.iter().map(signal_to_json).collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No CyberDots signals recorded.");
        return Ok(());
    }
    print_table(&rows);
    Ok(())
}

fn print_table(rows: &[CyberdotsSignal]) {
    println!(
        "{:<5}  {:<10}  {:<4}  {:<19}  {:<8}  {:<8}  {:<16}  Notes",
        "ID", "Symbol", "TF", "Recorded At", "Dot", "Track", "Flip",
    );
    println!("{}", "─".repeat(100));
    for r in rows {
        let flip = r.flip_from_prior.as_deref().unwrap_or("—");
        let notes = r.notes.as_deref().unwrap_or("");
        println!(
            "{:<5}  {:<10}  {:<4}  {:<19}  {:<8}  {:<8}  {:<16}  {}",
            r.id, r.symbol, r.timeframe, r.recorded_at, r.dot_state, r.trackline_position, flip, notes
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_since_accepts_date() {
        let s = normalize_since("2026-05-01").unwrap();
        assert_eq!(s, "2026-05-01");
    }

    #[test]
    fn normalize_since_accepts_day_window() {
        let s = normalize_since("30d").unwrap();
        // Format should be `YYYY-MM-DD HH:MM:SS`
        assert_eq!(s.len(), 19);
        assert!(s.chars().nth(4) == Some('-'));
    }

    #[test]
    fn normalize_since_accepts_hour_window() {
        let s = normalize_since("12h").unwrap();
        assert_eq!(s.len(), 19);
    }

    #[test]
    fn normalize_since_rejects_bad_unit() {
        assert!(normalize_since("30y").is_err());
    }

    #[test]
    fn normalize_since_rejects_empty() {
        assert!(normalize_since("").is_err());
        assert!(normalize_since("   ").is_err());
    }
}
