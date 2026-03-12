use anyhow::{bail, Result};
use serde_json::json;

use crate::db::agent_messages;
use crate::db::backend::BackendConnection;

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
    backend: &BackendConnection,
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
            let new_id = agent_messages::send_message_backend(
                backend,
                from_agent,
                to,
                priority,
                content,
                category,
                layer,
            )?;

            if json_output {
                let rows = agent_messages::list_messages_backend(
                    backend, None, None, None, false, None, None,
                )?;
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
            let rows = agent_messages::list_messages_backend(
                backend, from, to, layer, unacked, since, limit,
            )?;
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

        "reply" => {
            let parent_id = id.ok_or_else(|| anyhow::anyhow!("--id required"))?;
            let from_agent = from.ok_or_else(|| anyhow::anyhow!("--from required"))?;
            let content = value.ok_or_else(|| anyhow::anyhow!("reply content required"))?;
            let parent = agent_messages::get_message_by_id_backend(backend, parent_id)?
                .ok_or_else(|| anyhow::anyhow!("message #{} not found", parent_id))?;
            if let Some(l) = layer {
                validate_layer(l)?;
            }
            if let Some(p) = priority {
                validate_priority(p)?;
            }
            if let Some(c) = category {
                validate_category(c)?;
            }
            let to_agent = parent.from_agent.as_str();
            let body = format!("RE #{}: {}", parent_id, content);
            let new_id = agent_messages::send_message_backend(
                backend,
                from_agent,
                Some(to_agent),
                priority.or(Some("normal")),
                &body,
                category.or(Some("handoff")),
                layer.or(parent.layer.as_deref()),
            )?;
            if json_output {
                let inserted = agent_messages::get_message_by_id_backend(backend, new_id)?
                    .ok_or_else(|| anyhow::anyhow!("failed to load inserted message #{}", new_id))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "reply_to": parent_id,
                        "message": inserted
                    }))?
                );
            } else {
                println!("Replied to message #{} with #{}", parent_id, new_id);
            }
        }

        "flag" => {
            let parent_id = id.ok_or_else(|| anyhow::anyhow!("--id required"))?;
            let from_agent = from.ok_or_else(|| anyhow::anyhow!("--from required"))?;
            let parent = agent_messages::get_message_by_id_backend(backend, parent_id)?
                .ok_or_else(|| anyhow::anyhow!("message #{} not found", parent_id))?;
            if let Some(l) = layer {
                validate_layer(l)?;
            }
            if let Some(p) = priority {
                validate_priority(p)?;
            }
            if let Some(c) = category {
                validate_category(c)?;
            }
            let reason = value.unwrap_or("Data quality issue detected");
            let to_agent = parent.from_agent.as_str();
            let body = format!(
                "FLAG #{} from {}: {} | original: {}",
                parent_id, parent.from_agent, reason, parent.content
            );
            let new_id = agent_messages::send_message_backend(
                backend,
                from_agent,
                Some(to_agent),
                priority.or(Some("high")),
                &body,
                category.or(Some("escalation")),
                layer.or(parent.layer.as_deref()),
            )?;
            if json_output {
                let inserted = agent_messages::get_message_by_id_backend(backend, new_id)?
                    .ok_or_else(|| anyhow::anyhow!("failed to load inserted message #{}", new_id))?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "flagged_message_id": parent_id,
                        "message": inserted
                    }))?
                );
            } else {
                println!("Flagged message #{} with escalation #{}", parent_id, new_id);
            }
        }

        "ack" => {
            let msg_id = id.ok_or_else(|| anyhow::anyhow!("--id required"))?;
            agent_messages::acknowledge_backend(backend, msg_id)?;
            if json_output {
                println!("{}", serde_json::to_string_pretty(&json!({ "acked": msg_id }))?);
            } else {
                println!("Acknowledged message #{}", msg_id);
            }
        }

        "ack-all" => {
            let recipient = to.ok_or_else(|| anyhow::anyhow!("--to required for ack-all"))?;
            let count = agent_messages::acknowledge_all_backend(backend, recipient)?;
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
            let count = agent_messages::purge_old_backend(backend, n_days)?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "purged": count, "days": n_days }))?
                );
            } else {
                println!("Purged {} old acknowledged message(s)", count);
            }
        }

        _ => bail!("unknown agent-msg action '{}'. Valid: send, list, reply, flag, ack, ack-all, purge", action),
    }

    Ok(())
}
