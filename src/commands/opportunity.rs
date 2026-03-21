use anyhow::{bail, Result};
use chrono::Utc;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::opportunity_cost;

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    date: Option<&str>,
    asset: Option<&str>,
    missed_gain_pct: Option<f64>,
    missed_gain_usd: Option<f64>,
    avoided_loss_pct: Option<f64>,
    avoided_loss_usd: Option<f64>,
    rational: Option<bool>,
    notes: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "add" => {
            let event = value.ok_or_else(|| anyhow::anyhow!("event description required"))?;
            let entry_date = date
                .map(|d| d.to_string())
                .unwrap_or_else(|| Utc::now().format("%Y-%m-%d").to_string());
            let was_rational = rational.unwrap_or(true);

            let id = opportunity_cost::add_entry_backend(
                backend,
                &entry_date,
                event,
                asset,
                missed_gain_pct,
                missed_gain_usd,
                avoided_loss_pct,
                avoided_loss_usd,
                was_rational,
                notes,
            )?;

            if json_output {
                let rows = opportunity_cost::list_entries_backend(backend, None, None, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == id) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                }
            } else {
                println!("Added opportunity cost entry #{}", id);
            }
        }

        "list" => {
            let rows = opportunity_cost::list_entries_backend(backend, since, asset, limit)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "entries": rows, "count": rows.len() }))?
                );
            } else if rows.is_empty() {
                println!("No opportunity cost entries found.");
            } else {
                println!("Opportunity cost entries ({}):", rows.len());
                for row in rows {
                    println!(
                        "  #{} [{}] {} | missed=${:.2} avoided=${:.2} rational={}",
                        row.id,
                        row.date,
                        row.event,
                        row.missed_gain_usd.unwrap_or(0.0),
                        row.avoided_loss_usd.unwrap_or(0.0),
                        row.was_rational == 1
                    );
                }
            }
        }

        "stats" => {
            let stats = opportunity_cost::get_stats_backend(backend, since)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("Opportunity cost stats:");
                println!("  Entries: {}", stats.total_entries);
                println!("  Missed USD: ${:.2}", stats.total_missed_usd);
                println!("  Avoided USD: ${:.2}", stats.total_avoided_usd);
                println!("  Net USD: ${:.2}", stats.net_usd);
                println!("  Rational misses: {}", stats.rational_misses);
                println!("  Mistakes: {}", stats.mistakes);
            }
        }

        _ => bail!(
            "unknown opportunity action '{}'. Valid: add, list, stats",
            action
        ),
    }

    Ok(())
}
