use crate::db::{self, research_questions};
use anyhow::{bail, Result};
use serde_json::json;

fn validate_tilt(tilt: &str) -> Result<()> {
    match tilt {
        "neutral"
        | "leaning_bullish"
        | "leaning_bearish"
        | "strongly_bullish"
        | "strongly_bearish" => Ok(()),
        _ => bail!(
            "invalid tilt '{}'. Valid: neutral, leaning_bullish, leaning_bearish, strongly_bullish, strongly_bearish",
            tilt
        ),
    }
}

fn validate_status(status: &str) -> Result<()> {
    match status {
        "open" | "resolved" | "superseded" => Ok(()),
        _ => bail!("invalid status '{}'. Valid: open, resolved, superseded", status),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    action: &str,
    value: Option<&str>,
    id: Option<i64>,
    tilt: Option<&str>,
    evidence: Option<&str>,
    signal: Option<&str>,
    resolution: Option<&str>,
    status: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let conn = db::open_db(&db::default_db_path())?;

    match action {
        "add" => {
            let question = value.ok_or_else(|| anyhow::anyhow!("question text required"))?;
            let new_id = research_questions::add_question(&conn, question, signal)?;

            if json_output {
                let rows = research_questions::list_questions(&conn, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == new_id) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                }
            } else {
                println!("Added research question #{}", new_id);
            }
        }
        "list" => {
            if let Some(s) = status {
                validate_status(s)?;
            }
            let mut rows = research_questions::list_questions(&conn, status)?;

            if let Some(query) = value {
                let q = query.to_lowercase();
                rows.retain(|r| r.question.to_lowercase().contains(&q));
            }

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "questions": rows, "count": rows.len() }))?
                );
            } else if rows.is_empty() {
                println!("No research questions found.");
            } else {
                println!("Research questions ({}):", rows.len());
                for row in rows {
                    println!(
                        "  #{} [{}|{}] {}",
                        row.id, row.status, row.evidence_tilt, row.question
                    );
                    if let Some(sig) = row.key_signal.as_deref() {
                        println!("    signal: {}", sig);
                    }
                    if let Some(res) = row.resolution.as_deref() {
                        println!("    resolution: {}", res);
                    }
                }
            }
        }
        "update" => {
            let qid = id.ok_or_else(|| anyhow::anyhow!("--id required for update"))?;
            if let Some(t) = tilt {
                validate_tilt(t)?;
            }
            research_questions::update_question(&conn, qid, tilt, evidence, signal)?;

            if json_output {
                let rows = research_questions::list_questions(&conn, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == qid) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&json!({ "updated": qid }))?);
                }
            } else {
                println!("Updated research question #{}", qid);
            }
        }
        "resolve" => {
            let qid = id.ok_or_else(|| anyhow::anyhow!("--id required for resolve"))?;
            let res = resolution.ok_or_else(|| anyhow::anyhow!("--resolution required"))?;
            let st = status.unwrap_or("resolved");
            validate_status(st)?;
            research_questions::resolve_question(&conn, qid, res, st)?;

            if json_output {
                let rows = research_questions::list_questions(&conn, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == qid) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                } else {
                    println!("{}", serde_json::to_string_pretty(&json!({ "resolved": qid }))?);
                }
            } else {
                println!("Resolved research question #{} as {}", qid, st);
            }
        }
        _ => {
            bail!("unknown question action '{}'. Valid: add, list, update, resolve", action)
        }
    }

    Ok(())
}
