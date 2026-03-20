use anyhow::{bail, Result};
use chrono::Utc;
use serde_json::json;

use crate::db::agent_messages;
use crate::db::backend::BackendConnection;

fn validate_priority(priority: &str) -> Result<()> {
    match priority {
        "low" | "normal" | "high" | "critical" => Ok(()),
        _ => bail!(
            "invalid priority '{}'. Valid: low, normal, high, critical",
            priority
        ),
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
        _ => bail!(
            "invalid layer '{}'. Valid: low, medium, high, macro, cross",
            layer
        ),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    backend: &BackendConnection,
    action: &str,
    value: Option<&str>,
    batch: &[String],
    id: Option<i64>,
    ids: &[i64],
    package_id: Option<&str>,
    package_title: Option<&str>,
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

            let mut messages: Vec<String> = Vec::new();
            if let Some(content) = value {
                messages.push(content.to_string());
            }
            messages.extend(batch.iter().cloned());
            if messages.is_empty() {
                bail!("message content required (positional text or one/more --batch values)");
            }

            let effective_package_id =
                resolve_package_id(package_id, package_title, from_agent, messages.len());
            let mut ids = Vec::new();
            let mut inserted = Vec::new();
            for content in &messages {
                let new_id = agent_messages::send_message_backend(
                    backend,
                    from_agent,
                    to,
                    priority,
                    content,
                    category,
                    layer,
                    effective_package_id.as_deref(),
                    package_title,
                )?;
                ids.push(new_id);
                if json_output {
                    if let Some(row) = agent_messages::get_message_by_id_backend(backend, new_id)? {
                        inserted.push(row);
                    }
                }
            }

            if json_output {
                if messages.len() == 1 && batch.is_empty() {
                    if let Some(row) = inserted.into_iter().next() {
                        println!("{}", serde_json::to_string_pretty(&row)?);
                    }
                } else {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({
                            "package_id": effective_package_id,
                            "package_title": package_title,
                            "sent_count": ids.len(),
                            "ids": ids,
                            "messages": inserted,
                        }))?
                    );
                }
            } else if ids.len() == 1 {
                println!("Sent agent message #{}", ids[0]);
            } else {
                let joined = ids
                    .iter()
                    .map(|id| format!("#{}", id))
                    .collect::<Vec<_>>()
                    .join(", ");
                match (effective_package_id.as_deref(), package_title) {
                    (Some(pid), Some(title)) => {
                        println!(
                            "Sent {} agent messages in package {} ({}) : {}",
                            ids.len(),
                            pid,
                            title,
                            joined
                        );
                    }
                    (Some(pid), None) => {
                        println!(
                            "Sent {} agent messages in package {}: {}",
                            ids.len(),
                            pid,
                            joined
                        );
                    }
                    _ => println!("Sent {} agent messages: {}", ids.len(), joined),
                }
            }
        }

        "list" => {
            if let Some(l) = layer {
                validate_layer(l)?;
            }
            let rows = agent_messages::list_messages_backend(
                backend, from, to, layer, unacked, since, package_id, limit,
            )?;
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({ "messages": rows, "count": rows.len() })
                    )?
                );
            } else if rows.is_empty() {
                println!("No messages found.");
            } else {
                println!("Agent messages ({}):", rows.len());
                for row in rows {
                    let package = row
                        .package_id
                        .as_ref()
                        .map(|pid| match row.package_title.as_deref() {
                            Some(title) => format!(" [{}:{}]", pid, title),
                            None => format!(" [{}]", pid),
                        })
                        .unwrap_or_default();
                    println!(
                        "  #{} [{}|{}] {} -> {}{} | {}",
                        row.id,
                        row.priority,
                        row.layer.clone().unwrap_or_else(|| "-".to_string()),
                        row.from_agent,
                        row.to_agent
                            .clone()
                            .unwrap_or_else(|| "broadcast".to_string()),
                        package,
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
                parent.package_id.as_deref(),
                parent.package_title.as_deref(),
            )?;
            if json_output {
                let inserted = agent_messages::get_message_by_id_backend(backend, new_id)?
                    .ok_or_else(|| {
                        anyhow::anyhow!("failed to load inserted message #{}", new_id)
                    })?;
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
                parent.package_id.as_deref(),
                parent.package_title.as_deref(),
            )?;
            if json_output {
                let inserted = agent_messages::get_message_by_id_backend(backend, new_id)?
                    .ok_or_else(|| {
                        anyhow::anyhow!("failed to load inserted message #{}", new_id)
                    })?;
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
            // Collect IDs from both the bulk `ids` slice and the legacy single `id` field.
            let mut all_ids: Vec<i64> = ids.to_vec();
            if let Some(single) = id {
                if !all_ids.contains(&single) {
                    all_ids.push(single);
                }
            }
            if all_ids.is_empty() {
                anyhow::bail!("--id required (use --id N, repeatable for multiple)");
            }

            let mut acked = Vec::new();
            let mut errors = Vec::new();

            for msg_id in &all_ids {
                match agent_messages::acknowledge_backend(backend, *msg_id) {
                    Ok(()) => acked.push(*msg_id),
                    Err(e) => errors.push((*msg_id, e.to_string())),
                }
            }

            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "acked": acked,
                        "errors": errors.iter().map(|(id, err)| json!({"id": id, "error": err})).collect::<Vec<_>>(),
                    }))?
                );
            } else {
                for msg_id in &acked {
                    println!("Acknowledged message #{}", msg_id);
                }
                for (msg_id, err) in &errors {
                    eprintln!("⚠️  Failed to ack message #{}: {}", msg_id, err);
                }
            }

            if !errors.is_empty() && acked.is_empty() {
                anyhow::bail!("No messages were acknowledged");
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

        _ => bail!(
            "unknown agent-msg action '{}'. Valid: send, list, reply, flag, ack, ack-all, purge",
            action
        ),
    }

    Ok(())
}

fn resolve_package_id(
    package_id: Option<&str>,
    package_title: Option<&str>,
    from_agent: &str,
    message_count: usize,
) -> Option<String> {
    if let Some(existing) = package_id {
        return Some(existing.to_string());
    }
    if package_title.is_some() || message_count > 1 {
        let ts = Utc::now().format("%Y%m%d%H%M%S");
        return Some(format!(
            "pkg-{}-{}",
            ts,
            sanitize_package_fragment(from_agent)
        ));
    }
    None
}

fn sanitize_package_fragment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::agent_messages;
    use crate::db::backend::BackendConnection;

    fn setup_backend() -> BackendConnection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::run_migrations(&conn).unwrap();
        BackendConnection::Sqlite { conn }
    }

    #[test]
    fn test_bulk_ack_multiple_messages() {
        let backend = setup_backend();
        let id1 = agent_messages::send_message_backend(
            &backend, "agent-a", Some("agent-b"), None, "msg1", None, None, None, None,
        )
        .unwrap();
        let id2 = agent_messages::send_message_backend(
            &backend, "agent-a", Some("agent-b"), None, "msg2", None, None, None, None,
        )
        .unwrap();
        let id3 = agent_messages::send_message_backend(
            &backend, "agent-a", Some("agent-b"), None, "msg3", None, None, None, None,
        )
        .unwrap();

        run(
            &backend, "ack", None, &[], None, &[id1, id2, id3],
            None, None, None, None, None, None, None, false, None, None, None, false,
        )
        .unwrap();
    }

    #[test]
    fn test_bulk_ack_json_output() {
        let backend = setup_backend();
        let id1 = agent_messages::send_message_backend(
            &backend, "agent-a", Some("agent-b"), None, "msg1", None, None, None, None,
        )
        .unwrap();

        // Should not error
        run(
            &backend, "ack", None, &[], None, &[id1],
            None, None, None, None, None, None, None, false, None, None, None, true,
        )
        .unwrap();
    }

    #[test]
    fn test_ack_empty_ids_fails() {
        let backend = setup_backend();
        let result = run(
            &backend, "ack", None, &[], None, &[],
            None, None, None, None, None, None, None, false, None, None, None, false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ack_legacy_single_id() {
        let backend = setup_backend();
        let id = agent_messages::send_message_backend(
            &backend, "agent-a", Some("agent-b"), None, "msg", None, None, None, None,
        )
        .unwrap();

        // Legacy path: single id through the old Option parameter, empty ids slice
        run(
            &backend, "ack", None, &[], Some(id), &[],
            None, None, None, None, None, None, None, false, None, None, None, false,
        )
        .unwrap();
    }
}
