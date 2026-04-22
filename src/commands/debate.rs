use anyhow::{bail, Result};
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::debates;

/// Start a new adversarial debate.
pub fn start(
    backend: &BackendConnection,
    topic: &str,
    rounds: i64,
    json_output: bool,
) -> Result<()> {
    if topic.trim().is_empty() {
        bail!("topic cannot be empty");
    }
    if !(1..=10).contains(&rounds) {
        bail!("rounds must be between 1 and 10");
    }

    let id = debates::start_debate_backend(backend, topic, rounds)?;

    if json_output {
        let debate = debates::get_debate_view_backend(backend, id)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&debate_started_payload(id, debate))?
        );
    } else {
        println!("Started debate #{} — \"{}\" ({} rounds)", id, topic, rounds);
        println!("Use `pftui agent debate add-round` to add bull/bear arguments.");
    }
    Ok(())
}

/// Parameters for adding a debate round.
pub struct AddRoundParams<'a> {
    pub debate_id: i64,
    pub round_num: i64,
    pub position: &'a str,
    pub agent_source: Option<&'a str>,
    pub argument: &'a str,
    pub evidence: Option<&'a str>,
    pub json_output: bool,
}

/// Add a round argument (bull or bear) to an active debate.
pub fn add_round(backend: &BackendConnection, params: &AddRoundParams<'_>) -> Result<()> {
    debates::validate_position(params.position)?;

    // Check debate exists and is active
    let view = debates::get_debate_view_backend(backend, params.debate_id)?;
    match &view {
        None => bail!("debate #{} not found", params.debate_id),
        Some(v) if v.debate.status != "active" => {
            bail!("debate #{} is already resolved", params.debate_id)
        }
        _ => {}
    }

    let id = debates::add_round_backend(
        backend,
        params.debate_id,
        params.round_num,
        params.position,
        params.agent_source,
        params.argument,
        params.evidence,
    )?;

    if params.json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": "round_added",
                "round_id": id,
                "debate_id": params.debate_id,
                "round_num": params.round_num,
                "position": params.position,
            }))?
        );
    } else {
        println!(
            "Added {} argument to debate #{}, round {}",
            params.position.to_uppercase(),
            params.debate_id,
            params.round_num
        );
    }
    Ok(())
}

/// Resolve (close) a debate with an optional summary.
pub fn resolve(
    backend: &BackendConnection,
    debate_id: i64,
    summary: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let existing = debates::get_debate_view_backend(backend, debate_id)?;
    match &existing {
        None => bail!("debate #{} not found", debate_id),
        Some(v) if v.debate.status == "resolved" => {
            bail!("debate #{} is already resolved", debate_id)
        }
        _ => {}
    }

    debates::resolve_debate_backend(backend, debate_id, summary)?;

    if json_output {
        let updated = debates::get_debate_view_backend(backend, debate_id)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&debate_resolved_payload(debate_id, updated))?
        );
    } else {
        println!("Resolved debate #{}", debate_id);
        if let Some(s) = summary {
            println!("Summary: {}", s);
        }
    }
    Ok(())
}

/// List debates with optional filters.
pub fn history(
    backend: &BackendConnection,
    status: Option<&str>,
    topic: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    if let Some(s) = status {
        debates::validate_status(s)?;
    }

    let items = debates::list_debates_backend(backend, status, topic, limit)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if items.is_empty() {
        println!("No debates found.");
    } else {
        println!("{:<4} {:<10} {:<6} {:<20} Topic", "ID", "Status", "Rnds", "Created");
        println!("{}", "-".repeat(70));
        for d in &items {
            let created_short = d.created_at.get(..16).unwrap_or(&d.created_at);
            println!(
                "{:<4} {:<10} {:<6} {:<20} {}",
                d.id,
                d.status,
                d.max_rounds,
                created_short,
                truncate_str(&d.topic, 40),
            );
        }
        println!("\n{} debate(s)", items.len());
    }
    Ok(())
}

/// Show full debate detail with all rounds.
pub fn summary(
    backend: &BackendConnection,
    debate_id: Option<i64>,
    json_output: bool,
) -> Result<()> {
    // If no ID given, show the latest debate
    let id = match debate_id {
        Some(id) => id,
        None => {
            let items = debates::list_debates_backend(backend, None, None, Some(1))?;
            match items.first() {
                Some(d) => d.id,
                None => bail!("no debates found"),
            }
        }
    };

    let view = debates::get_debate_view_backend(backend, id)?;
    match view {
        None => bail!("debate #{} not found", id),
        Some(v) => {
            if json_output {
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!("━━━ Debate #{}: {} ━━━", v.debate.id, v.debate.topic);
                println!(
                    "Status: {}  |  Rounds: {}/{}  |  Created: {}",
                    v.debate.status, v.round_count, v.debate.max_rounds, v.debate.created_at
                );
                if let Some(ref resolved) = v.debate.resolved_at {
                    println!("Resolved: {}", resolved);
                }
                if let Some(ref summary) = v.debate.resolution_summary {
                    println!("Resolution: {}", summary);
                }
                println!();

                let mut current_round = 0i64;
                for r in &v.rounds {
                    if r.round_num != current_round {
                        current_round = r.round_num;
                        println!("── Round {} ──", current_round);
                    }
                    let icon = if r.position == "bull" {
                        "🐂"
                    } else {
                        "🐻"
                    };
                    let agent = r
                        .agent_source
                        .as_deref()
                        .unwrap_or("unattributed");
                    println!(
                        "\n  {} {} ({})",
                        icon,
                        r.position.to_uppercase(),
                        agent
                    );
                    // Wrap argument text at ~72 chars for readability
                    for line in textwrap(&r.argument_text, 72) {
                        println!("    {}", line);
                    }
                    if let Some(ref ev) = r.evidence_refs {
                        println!("    Evidence: {}", ev);
                    }
                }
                println!();
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn debate_started_payload(debate_id: i64, debate: Option<debates::DebateView>) -> serde_json::Value {
    json!({
        "action": "debate_started",
        "debate_id": debate_id,
        "debate": debate,
    })
}

fn debate_resolved_payload(
    debate_id: i64,
    debate: Option<debates::DebateView>,
) -> serde_json::Value {
    json!({
        "action": "debate_resolved",
        "debate_id": debate_id,
        "debate": debate,
    })
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

fn textwrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let words: Vec<&str> = paragraph.split_whitespace().collect();
        if words.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in words {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() > width {
                lines.push(current);
                current = word.to_string();
            } else {
                current.push(' ');
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::backend::BackendConnection;
    use rusqlite::Connection;

    fn setup_backend() -> BackendConnection {
        BackendConnection::Sqlite {
            conn: Connection::open_in_memory().unwrap(),
        }
    }

    #[test]
    fn debate_started_payload_includes_top_level_debate_id() {
        let backend = setup_backend();
        let debate_id = debates::start_debate_backend(&backend, "BTC to 200k?", 3).unwrap();
        let debate = debates::get_debate_view_backend(&backend, debate_id).unwrap();

        let payload = debate_started_payload(debate_id, debate);

        assert_eq!(payload["action"], "debate_started");
        assert_eq!(payload["debate_id"], debate_id);
        assert_eq!(payload["debate"]["id"], debate_id);
    }

    #[test]
    fn debate_resolved_payload_includes_top_level_debate_id() {
        let backend = setup_backend();
        let debate_id = debates::start_debate_backend(&backend, "US recession in 2026?", 2).unwrap();
        debates::resolve_debate_backend(&backend, debate_id, Some("Bull case failed")).unwrap();
        let debate = debates::get_debate_view_backend(&backend, debate_id).unwrap();

        let payload = debate_resolved_payload(debate_id, debate);

        assert_eq!(payload["action"], "debate_resolved");
        assert_eq!(payload["debate_id"], debate_id);
        assert_eq!(payload["debate"]["id"], debate_id);
        assert_eq!(payload["debate"]["status"], "resolved");
    }
}
