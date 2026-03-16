use anyhow::{bail, Result};
use chrono::NaiveDate;
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::consensus;

fn validate_date(date: &str) -> Result<()> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| anyhow::anyhow!("invalid date '{}': expected YYYY-MM-DD", date))
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    source: Option<&str>,
    topic: Option<&str>,
    call_text: Option<&str>,
    date: Option<&str>,
    limit: usize,
    json_output: bool,
) -> Result<()> {
    match action {
        "add" => {
            let source = source.ok_or_else(|| anyhow::anyhow!("--source is required"))?;
            let topic = topic.ok_or_else(|| anyhow::anyhow!("--topic is required"))?;
            let call_text = call_text.ok_or_else(|| anyhow::anyhow!("--call is required"))?;
            let date = date.ok_or_else(|| anyhow::anyhow!("--date is required"))?;
            validate_date(date)?;

            let id = consensus::add_call_backend(backend, source, topic, call_text, date)?;
            let row = consensus::list_calls_backend(backend, Some(topic), Some(source), limit)?
                .into_iter()
                .find(|row| row.id == id)
                .ok_or_else(|| anyhow::anyhow!("failed to reload saved consensus call"))?;

            if json_output {
                println!("{}", serde_json::to_string_pretty(&row)?);
            } else {
                println!("Added consensus call #{} [{} / {}]", id, source, topic);
            }
        }
        "list" => {
            let rows = consensus::list_calls_backend(backend, topic, source, limit)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "calls": rows,
                        "count": rows.len(),
                    }))?
                );
            } else if rows.is_empty() {
                println!("No consensus calls found.");
            } else {
                println!("Consensus calls ({}):", rows.len());
                for row in rows {
                    println!(
                        "  #{} [{}] {} | {} | {}",
                        row.id, row.call_date, row.source, row.topic, row.call_text
                    );
                }
            }
        }
        _ => bail!("unknown consensus action '{}'. Valid: add, list", action),
    }

    Ok(())
}
