use anyhow::{bail, Result};
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::user_predictions;

fn validate_conviction(value: &str) -> Result<()> {
    match value {
        "high" | "medium" | "low" => Ok(()),
        _ => bail!("invalid conviction '{}'. Valid: high, medium, low", value),
    }
}

fn validate_outcome(value: &str) -> Result<()> {
    match value {
        "pending" | "correct" | "partial" | "wrong" => Ok(()),
        _ => bail!("invalid outcome '{}'. Valid: pending, correct, partial, wrong", value),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    id: Option<i64>,
    symbol: Option<&str>,
    conviction: Option<&str>,
    target_date: Option<&str>,
    outcome: Option<&str>,
    notes: Option<&str>,
    filter: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    match action {
        "add" => {
            let claim = value.ok_or_else(|| anyhow::anyhow!("prediction claim required"))?;
            if let Some(c) = conviction {
                validate_conviction(c)?;
            }
            let new_id = user_predictions::add_prediction_backend(
                backend,
                claim,
                symbol,
                conviction,
                target_date,
            )?;

            if json_output {
                let rows = user_predictions::list_predictions_backend(backend, None, None, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == new_id) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                }
            } else {
                println!("Added prediction #{}", new_id);
            }
        }

        "list" => {
            if let Some(f) = filter {
                validate_outcome(f)?;
            }
            let rows = user_predictions::list_predictions_backend(backend, filter, symbol, limit)?;

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "predictions": rows, "count": rows.len() }))?
                );
            } else if rows.is_empty() {
                println!("No predictions found.");
            } else {
                println!("Predictions ({}):", rows.len());
                for row in rows {
                    let sym = row.symbol.unwrap_or_else(|| "—".to_string());
                    println!(
                        "  #{} [{}|{}|{}] {}",
                        row.id, sym, row.conviction, row.outcome, row.claim
                    );
                }
            }
        }

        "score" => {
            let pid = id.ok_or_else(|| anyhow::anyhow!("--id required"))?;
            let out = outcome.ok_or_else(|| anyhow::anyhow!("--outcome required"))?;
            validate_outcome(out)?;
            user_predictions::score_prediction_backend(backend, pid, out, notes)?;

            if json_output {
                let rows = user_predictions::list_predictions_backend(backend, None, None, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == pid) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&json!({ "scored": pid }))?);
                }
            } else {
                println!("Scored prediction #{} as {}", pid, out);
            }
        }

        "stats" => {
            let stats = user_predictions::get_stats_backend(backend)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("Prediction stats:");
                println!("  Total: {}", stats.total);
                println!("  Scored: {}", stats.scored);
                println!("  Pending: {}", stats.pending);
                println!("  Correct: {}", stats.correct);
                println!("  Partial: {}", stats.partial);
                println!("  Wrong: {}", stats.wrong);
                println!("  Hit rate: {:.1}%", stats.hit_rate_pct);
            }
        }

        _ => bail!("unknown predict action '{}'. Valid: add, list, score, stats", action),
    }

    Ok(())
}
