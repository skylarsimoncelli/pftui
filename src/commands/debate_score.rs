//! `pftui analytics debate-score` — track which side (bull/bear) was right
//! historically for each debated topic. Feeds into system accuracy tracking.
//!
//! Subcommands:
//!   add     — score a resolved debate (which side won, what actually happened)
//!   list    — list scored debates with optional filters
//!   accuracy — aggregate accuracy stats (bull vs bear win rates, by topic)
//!   unscored — list resolved debates that haven't been scored yet

use anyhow::{bail, Result};
use serde_json::json;

use crate::db::backend::BackendConnection;
use crate::db::debate_scores;
use crate::db::debates;

/// Parameters for scoring a debate.
pub struct ScoreParams<'a> {
    pub debate_id: i64,
    pub winner: &'a str,
    pub margin: &'a str,
    pub actual_outcome: &'a str,
    pub argument_assessment: Option<&'a str>,
    pub scored_by: Option<&'a str>,
    pub json_output: bool,
}

/// Score a resolved debate: which side was right?
pub fn add(backend: &BackendConnection, params: &ScoreParams<'_>) -> Result<()> {
    let debate_id = params.debate_id;
    let winner = params.winner;
    let margin = params.margin;
    let actual_outcome = params.actual_outcome;
    let argument_assessment = params.argument_assessment;
    let scored_by = params.scored_by;
    let json_output = params.json_output;
    // Validate the debate exists and is resolved
    let view = debates::get_debate_view_backend(backend, debate_id)?;
    match &view {
        None => bail!("debate #{} not found", debate_id),
        Some(v) if v.debate.status != "resolved" => {
            bail!(
                "debate #{} is still active — resolve it first with `agent debate resolve`",
                debate_id
            )
        }
        _ => {}
    }

    let id = debate_scores::score_debate_backend(
        backend,
        debate_id,
        winner,
        margin,
        actual_outcome,
        argument_assessment,
        scored_by,
    )?;

    if json_output {
        let score = debate_scores::get_score_backend(backend, debate_id)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "action": "debate_scored",
                "score_id": id,
                "debate_id": debate_id,
                "score": score,
            }))?
        );
    } else {
        let icon = match winner {
            "bull" => "🐂",
            "bear" => "🐻",
            _ => "⚖️",
        };
        println!(
            "Scored debate #{}: {} {} wins ({})",
            debate_id,
            icon,
            winner.to_uppercase(),
            margin
        );
        println!("Outcome: {}", actual_outcome);
    }
    Ok(())
}

/// List scored debates with optional filters.
pub fn list(
    backend: &BackendConnection,
    topic: Option<&str>,
    winner: Option<&str>,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    if let Some(w) = winner {
        debate_scores::validate_winner(w)?;
    }

    let items = debate_scores::list_scored_debates_backend(backend, topic, winner, limit)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if items.is_empty() {
        println!("No scored debates found.");
    } else {
        println!(
            "{:<4} {:<8} {:<10} {:<20} Topic",
            "ID", "Winner", "Margin", "Scored"
        );
        println!("{}", "-".repeat(72));
        for d in &items {
            let icon = match d.score.winner.as_str() {
                "bull" => "🐂",
                "bear" => "🐻",
                _ => "⚖️",
            };
            let scored_short = d.score.scored_at.get(..16).unwrap_or(&d.score.scored_at);
            let topic_display = if d.topic.len() > 35 {
                format!("{}…", &d.topic[..34])
            } else {
                d.topic.clone()
            };
            println!(
                "{:<4} {} {:<6} {:<10} {:<20} {}",
                d.debate_id, icon, d.score.winner, d.score.margin, scored_short, topic_display,
            );
        }
        println!("\n{} scored debate(s)", items.len());
    }
    Ok(())
}

/// Show aggregate accuracy statistics.
pub fn accuracy(
    backend: &BackendConnection,
    topic: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let acc = debate_scores::compute_accuracy_backend(backend, topic)?;

    if json_output {
        let mut obj = json!({
            "accuracy": acc,
        });
        if let Some(t) = topic {
            obj["topic_filter"] = json!(t);
        }
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else if acc.total_scored == 0 {
        println!("No scored debates found.");
        if topic.is_some() {
            println!("Try without --topic filter, or score some debates first.");
        }
    } else {
        println!("━━━ Debate Accuracy ━━━");
        if let Some(t) = topic {
            println!("Filter: topics containing \"{}\"", t);
        }
        println!();
        println!("Total scored:    {}", acc.total_scored);
        println!(
            "🐂 Bull wins:    {} ({:.1}%)",
            acc.bull_wins, acc.bull_win_rate_pct
        );
        println!(
            "🐻 Bear wins:    {} ({:.1}%)",
            acc.bear_wins, acc.bear_win_rate_pct
        );
        println!("⚖️  Mixed:        {}", acc.mixed);
        println!();
        println!("Decisive calls:  {}", acc.decisive_count);
        println!("Marginal calls:  {}", acc.marginal_count);
    }
    Ok(())
}

/// List resolved debates that haven't been scored yet.
pub fn unscored(
    backend: &BackendConnection,
    limit: Option<usize>,
    json_output: bool,
) -> Result<()> {
    let items = debate_scores::list_unscored_backend(backend, limit)?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else if items.is_empty() {
        println!("All resolved debates have been scored. ✓");
    } else {
        println!("{:<4} {:<20} Topic", "ID", "Resolved");
        println!("{}", "-".repeat(60));
        for d in &items {
            let resolved_short = d
                .resolved_at
                .as_deref()
                .and_then(|s| s.get(..16))
                .unwrap_or("—");
            let topic_display = if d.topic.len() > 40 {
                format!("{}…", &d.topic[..39])
            } else {
                d.topic.clone()
            };
            println!("{:<4} {:<20} {}", d.id, resolved_short, topic_display);
        }
        println!("\n{} unscored debate(s)", items.len());
    }
    Ok(())
}
