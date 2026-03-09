use anyhow::{bail, Result};
use rusqlite::Connection;
use serde_json::json;

use crate::db::timeframe_signals;

pub fn run(
    conn: &Connection,
    action: &str,
    symbol: Option<&str>,
    signal_type: Option<&str>,
    severity: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "signals" => {
            let mut rows =
                timeframe_signals::list_signals(conn, signal_type, severity, limit.or(Some(25)))?;
            if let Some(sym) = symbol {
                let needle = format!("\"{}\"", sym.to_uppercase());
                rows.retain(|r| r.assets.to_uppercase().contains(&needle));
            }

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "signals": rows,
                        "count": rows.len()
                    }))?
                );
            } else if rows.is_empty() {
                println!("No cross-timeframe signals found.");
            } else {
                println!("Cross-timeframe signals ({}):", rows.len());
                for sig in rows {
                    println!(
                        "  [{}|{}] {}\n    assets={} layers={} at={}",
                        sig.severity,
                        sig.signal_type,
                        sig.description,
                        sig.assets,
                        sig.layers,
                        sig.detected_at
                    );
                }
            }
        }
        "summary" | "low" | "medium" | "high" | "macro" | "alignment" => {
            bail!(
                "analytics '{}' is not implemented yet. Available now: analytics signals",
                action
            )
        }
        _ => {
            bail!(
                "unknown analytics action '{}'. Valid: signals, summary, low, medium, high, macro, alignment",
                action
            )
        }
    }

    Ok(())
}
