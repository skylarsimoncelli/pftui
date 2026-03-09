use anyhow::{bail, Result};
use serde_json::json;

use crate::db::{self, agent_messages};

fn validate_priority(priority: &str) -> Result<()> {
    match priority {
        "low" | "normal" | "high" | "critical" => Ok(()),
        _ => bail!("invalid priority '{}'. Valid: low, normal, high, critical", priority),
    }
}

fn validate_category(category: &str) -> Result<()> {
    match category {
        "signal" | "feedback" | "alert" | "handoff" | "escalation" => Ok(()),
        _ => bail!(
            "invalid category '{}'. Valid: signal, feedback, alert, handoff, escalation",
            category
        ),
    }
}

fn validate_layer(layer: &str) -> Result<()> {
    match layer {
        "low" | "medium" | "high" | "macro" | "cross" => Ok(()),
        _ => bail!("invalid layer '{}'. Valid: low, medium, high, macro, cross", layer),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    action: &str,
    value: Option<&str>,
    id: Option<i64>,
    from: Option<&str>,
    to: Option<&str>,
    priority: Option<&str>,
    category: Option<&str>,
    layer: Option<&str>,
    unacked: bool,
    since: Option<&str>,
    days: Option<usize>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let conn = db::open_db(&db::default_db_path())?;

    match action {
        "send" => {
            let content = value.ok_or_else(|| anyhow::anyhow!("message content required"))?;
            let from_agent = from.ok_or_else(|| anyhow::anyhow!("--from required"))?;
            if let Some(p) = priority {
                validate_priority(p)?;
            }
            if let Some(c) = category {
                validate_category(c)?;
            }
            if let Some(l) = layer {
                validate_layer(l)?;
            }
            let new_id = agent_messages::send_message(
                &conn,
                from_agent,
                to,
                priority,
                content,
                category,
                layer,
            )?;

            if json_output {
                let rows = agent_messages::list_messages(&conn, None, None, false, None, None)?;
                if let Some(row) = rows.into_iter().find(|r| r.id == new_id) {
                    println!("{}", serde_json::to_string_pretty(&row)?);
                }
            } else {
                println!("Sent agent message #{}", new_id);
            }
        }

        "list" => {
            if let Some(l) = layer {
                validate_layer(l)?;
            }
            let rows = agent_messages::list_messages(&conn, to, layer, unacked, since, limit)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "messages": rows, "count": rows.len() }))?
                );
            } else if rows.is_empty() {
                println!("No messages found.");
            } else {
                println!("Agent messages ({}):", rows.len());
                for row in rows {
                    println!(
                        "  #{} [{}|{}] {} -> {} | {}",
                        row.id,
                        row.priority,
                        row.layer.clone().unwrap_or_else(|| "-".to_string()),
                        row.from_agent,
                        row.to_agent.clone().unwrap_or_else(|| "broadcast".to_string()),
                        row.content
                    );
                }
            }
        }

        "ack" => {
            let msg_id = id.ok_or_else(|| anyhow::anyhow!("--id required"))?;
            agent_messages::acknowledge(&conn, msg_id)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "acked": msg_id }))?);
            } else {
                println!("Acknowledged message #{}", msg_id);
            }
        }

        "ack-all" => {
            let recipient = to.ok_or_else(|| anyhow::anyhow!("--to required for ack-all"))?;
            let count = agent_messages::acknowledge_all(&conn, recipient)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "acked": count, "to": recipient }))?
                );
            } else {
                println!("Acknowledged {} message(s) for {}", count, recipient);
            }
        }

        "purge" => {
            let n_days = days.unwrap_or(30);
            let count = agent_messages::purge_old(&conn, n_days)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "purged": count, "days": n_days }))?
                );
            } else {
                println!("Purged {} old acknowledged message(s)", count);
            }
        }

        _ => bail!("unknown agent-msg action '{}'. Valid: send, list, ack, ack-all, purge", action),
    }

    Ok(())
}
